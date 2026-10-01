//! Turn-boundary rotation across ChatGPT accounts kept under `[account_rotation].accounts_dir`.
//!
//! Each subdirectory of `accounts_dir` is one account home holding its own `auth.json`.
//! Before a turn starts, the session checks whether the current account still has weekly
//! quota above the reserve; if not, it switches the process-wide `AuthManager` to the
//! usable account whose weekly window resets first ("fill-first"). Switching never happens
//! mid-turn, and the rest of Codex reacts to the auth owner change on its own.

use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::Hash;
use std::hash::Hasher;
use std::path::Path;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use chrono::DateTime;
use chrono::Utc;
use codex_backend_client::Client as BackendClient;
use codex_config::types::AccountRotationConfig;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_protocol::error::CodexErr;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::RateLimitSnapshot;
use codex_protocol::protocol::RateLimitWindow;
use codex_protocol::protocol::SessionSource;
use tracing::info;
use tracing::warn;

/// An account directory containing this file is never selected.
pub(crate) const DISABLED_MARKER: &str = "disabled";
const AUTH_FILE: &str = "auth.json";
const CODEX_LIMIT_ID: &str = "codex";
/// How long an account stays excluded after a usage-limit error without a known reset time.
const EXHAUSTED_FALLBACK: Duration = Duration::from_secs(5 * 60);

/// Quota view of one account derived from a rate-limit snapshot.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AccountUsage {
    /// Remaining percentage of the longest (weekly) window.
    pub(crate) weekly_remaining_percent: f64,
    /// Unix seconds when the weekly window resets, when known.
    pub(crate) weekly_resets_at: Option<i64>,
    /// Some window is fully consumed or the backend reports the limit as reached.
    pub(crate) blocked: bool,
}

impl AccountUsage {
    pub(crate) fn from_snapshot(snapshot: &RateLimitSnapshot) -> Option<Self> {
        let windows: Vec<&RateLimitWindow> =
            [snapshot.primary.as_ref(), snapshot.secondary.as_ref()]
                .into_iter()
                .flatten()
                .collect();
        // Prefer the longest window; without durations the secondary window is the weekly one.
        let weekly = windows
            .iter()
            .copied()
            .filter(|window| window.window_minutes.is_some())
            .max_by_key(|window| window.window_minutes)
            .or(snapshot.secondary.as_ref())
            .or(snapshot.primary.as_ref())?;
        Some(Self {
            weekly_remaining_percent: (100.0 - weekly.used_percent).clamp(0.0, 100.0),
            weekly_resets_at: weekly.resets_at,
            blocked: snapshot.rate_limit_reached_type.is_some()
                || windows.iter().any(|window| window.used_percent >= 100.0),
        })
    }

    pub(crate) fn is_usable(&self, reserve_percent: u8) -> bool {
        !self.blocked && self.weekly_remaining_percent > f64::from(reserve_percent)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AccountCandidate {
    pub(crate) name: String,
    pub(crate) dir: PathBuf,
    /// `None` when usage could not be determined; such accounts are never selected.
    pub(crate) usage: Option<AccountUsage>,
}

/// Lists `accounts_dir/*/auth.json`, sorted by directory name, skipping disabled or
/// unreadable accounts.
pub(crate) fn list_accounts(accounts_dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(accounts_dir) else {
        return Vec::new();
    };
    let mut accounts: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|dir| dir.is_dir() && !dir.join(DISABLED_MARKER).exists())
        .filter(|dir| {
            std::fs::read_to_string(dir.join(AUTH_FILE))
                .ok()
                .is_some_and(|text| serde_json::from_str::<serde_json::Value>(&text).is_ok())
        })
        .filter_map(|dir| Some((dir.file_name()?.to_string_lossy().into_owned(), dir)))
        .collect();
    accounts.sort();
    accounts
}

/// Fill-first selection: keep `current` while it is usable, otherwise pick the usable
/// account whose weekly window resets earliest (ties by name). `None` when nothing is usable.
pub(crate) fn select_account<'a>(
    candidates: &'a [AccountCandidate],
    current: Option<&Path>,
    reserve_percent: u8,
) -> Option<&'a AccountCandidate> {
    let usable = candidates.iter().filter(|candidate| {
        candidate
            .usage
            .as_ref()
            .is_some_and(|usage| usage.is_usable(reserve_percent))
    });
    if let Some(current) = current
        && let Some(candidate) = usable
            .clone()
            .find(|candidate| same_dir(&candidate.dir, current))
    {
        return Some(candidate);
    }
    usable.min_by(|a, b| {
        let reset = |candidate: &AccountCandidate| {
            candidate
                .usage
                .as_ref()
                .and_then(|usage| usage.weekly_resets_at)
                .unwrap_or(i64::MAX)
        };
        reset(a).cmp(&reset(b)).then_with(|| a.name.cmp(&b.name))
    })
}

fn same_dir(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (a.canonicalize(), b.canonicalize()),
            (Ok(a), Ok(b)) if a == b
        )
}

fn account_name(accounts_dir: &Path, dir: &Path) -> String {
    match (dir.parent(), dir.file_name()) {
        (Some(parent), Some(name)) if same_dir(parent, accounts_dir) => {
            name.to_string_lossy().into_owned()
        }
        _ => dir.display().to_string(),
    }
}

/// A completed switch, reported to the user as one line.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AccountSwitch {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) remaining_percent: f64,
}

impl AccountSwitch {
    pub(crate) fn notice(&self) -> String {
        format!(
            "account: switched {} → {} (remaining {:.0}%)",
            self.from, self.to, self.remaining_percent
        )
    }
}

/// Process-wide rotation state shared by every session using the same `AuthManager`.
#[derive(Default)]
struct SharedState {
    usage: HashMap<PathBuf, (Instant, Option<AccountUsage>)>,
    exhausted_until: HashMap<PathBuf, Instant>,
}

static SHARED: LazyLock<Mutex<SharedState>> = LazyLock::new(Mutex::default);
/// Serializes rotation decisions so concurrent sessions do not race each other's switch.
static ROTATION_PERMIT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

fn with_shared<T>(f: impl FnOnce(&mut SharedState) -> T) -> T {
    match SHARED.lock() {
        Ok(mut state) => f(&mut state),
        Err(poisoned) => f(&mut poisoned.into_inner()),
    }
}

/// Excludes `dir` until its limit resets so the next turn rotates away from it.
pub(crate) fn mark_exhausted(dir: &Path, resets_at: Option<DateTime<Utc>>) {
    let now = Instant::now();
    let until = resets_at
        .and_then(|resets_at| (resets_at - Utc::now()).to_std().ok())
        .map_or(now + EXHAUSTED_FALLBACK, |remaining| now + remaining);
    with_shared(|state| {
        state.usage.remove(dir);
        state.exhausted_until.insert(dir.to_path_buf(), until);
    });
}

fn is_exhausted(dir: &Path) -> bool {
    let now = Instant::now();
    with_shared(|state| {
        state.exhausted_until.retain(|_, until| *until > now);
        state.exhausted_until.contains_key(dir)
    })
}

/// Only sessions that own their turns rotate; sub-agents run inside a parent's turn.
pub(crate) fn rotation_applies_to(session_source: &SessionSource) -> bool {
    !matches!(
        session_source,
        SessionSource::SubAgent(_) | SessionSource::Internal(_)
    )
}

async fn cached_usage(
    auth_manager: &AuthManager,
    dir: &Path,
    chatgpt_base_url: &str,
    cache_for: Duration,
) -> Option<AccountUsage> {
    if let Some(usage) = with_shared(|state| {
        state
            .usage
            .get(dir)
            .filter(|(fetched_at, _)| fetched_at.elapsed() < cache_for)
            .map(|(_, usage)| usage.clone())
    }) {
        return usage;
    }
    let usage = fetch_usage(auth_manager, dir, chatgpt_base_url).await;
    with_shared(|state| {
        state
            .usage
            .insert(dir.to_path_buf(), (Instant::now(), usage.clone()))
    });
    usage
}

async fn fetch_usage(
    auth_manager: &AuthManager,
    dir: &Path,
    chatgpt_base_url: &str,
) -> Option<AccountUsage> {
    let auth = auth_manager.load_auth_for_home(dir).await?;
    if !matches!(auth, CodexAuth::Chatgpt(_)) {
        return None;
    }
    let client = BackendClient::from_auth(
        chatgpt_base_url.to_string(),
        &auth,
        auth_manager.http_client_factory(),
    );
    let response = match client.get_rate_limits_with_reset_credits().await {
        Ok(response) => response,
        Err(err) => {
            warn!(account = %dir.display(), %err, "account rotation: failed to read usage");
            return None;
        }
    };
    let snapshot = response
        .rate_limits
        .iter()
        .find(|snapshot| snapshot.limit_id.as_deref() == Some(CODEX_LIMIT_ID))
        .or_else(|| response.rate_limits.first())?;
    let mut usage = AccountUsage::from_snapshot(snapshot)?;
    usage.blocked |= response.ordinary_usage_allowed == Some(false);
    Some(usage)
}

/// Switches accounts when the current one is no longer usable. `latest` is the current
/// account's most recent rate-limit snapshot from response headers, when known.
pub(crate) async fn maybe_rotate(
    config: &AccountRotationConfig,
    auth_manager: &AuthManager,
    latest: Option<&RateLimitSnapshot>,
    chatgpt_base_url: &str,
) -> Option<AccountSwitch> {
    if auth_manager.has_external_auth()
        || !matches!(auth_manager.auth_cached(), Some(CodexAuth::Chatgpt(_)))
    {
        return None;
    }
    let Ok(_rotation_permit) = ROTATION_PERMIT.acquire().await else {
        return None;
    };
    let accounts_dir = config.accounts_dir.as_path();
    let cache_for = Duration::from_secs(config.usage_cache_seconds);
    let current_dir = auth_manager.auth_home();
    let current_usage = if is_exhausted(&current_dir) {
        None
    } else if let Some(usage) = latest
        .filter(|snapshot| {
            snapshot
                .limit_id
                .as_deref()
                .is_none_or(|id| id == CODEX_LIMIT_ID)
        })
        .and_then(AccountUsage::from_snapshot)
    {
        Some(usage)
    } else {
        cached_usage(auth_manager, &current_dir, chatgpt_base_url, cache_for).await
    };
    let current_usable = !is_exhausted(&current_dir)
        && current_usage
            .as_ref()
            .is_none_or(|usage| usage.is_usable(config.reserve_percent));
    if current_usable {
        return None;
    }

    let accounts = list_accounts(accounts_dir);
    let candidates = futures::future::join_all(accounts.into_iter().map(|(name, dir)| {
        let current_usage = current_usage.clone();
        let current_dir = current_dir.clone();
        async move {
            let usage = if same_dir(&dir, &current_dir) {
                current_usage.filter(|_| !is_exhausted(&dir))
            } else if is_exhausted(&dir) {
                None
            } else {
                cached_usage(auth_manager, &dir, chatgpt_base_url, cache_for).await
            };
            AccountCandidate { name, dir, usage }
        }
    }))
    .await;

    let Some(selected) = select_account(&candidates, Some(&current_dir), config.reserve_percent)
    else {
        info!("account rotation: no usable account; keeping the current account");
        return None;
    };
    if same_dir(&selected.dir, &current_dir) {
        return None;
    }
    let switch = AccountSwitch {
        from: account_name(accounts_dir, &current_dir),
        to: selected.name.clone(),
        remaining_percent: selected
            .usage
            .as_ref()
            .map_or(0.0, |usage| usage.weekly_remaining_percent),
    };
    let owner_changed = auth_manager.switch_home(selected.dir.clone()).await;
    info!(
        from = %switch.from,
        to = %switch.to,
        owner_changed,
        "account rotation: switched account"
    );
    Some(switch)
}

/// Whether a request failed because encrypted reasoning or compaction content issued to a
/// different account could not be decrypted.
pub(crate) fn is_invalid_encrypted_content_error(err: &CodexErr) -> bool {
    let text = match err.details() {
        CodexErrorDetails::InvalidRequest(text) | CodexErrorDetails::Stream(text) => text.as_str(),
        CodexErrorDetails::UnexpectedStatus(error) => error.body.as_str(),
        _ => return false,
    };
    if text.contains("invalid_encrypted_content") {
        return true;
    }
    let text = text.to_ascii_lowercase();
    text.contains("encrypted content")
        && [
            "could not be verified",
            "could not be decrypted",
            "could not be parsed",
        ]
        .iter()
        .any(|phrase| text.contains(phrase))
}

fn encrypted_blob(item: &ResponseItem) -> Option<&str> {
    match item {
        ResponseItem::Reasoning {
            encrypted_content: Some(blob),
            ..
        }
        | ResponseItem::Compaction {
            encrypted_content: blob,
            ..
        }
        | ResponseItem::ContextCompaction {
            encrypted_content: Some(blob),
            ..
        } => Some(blob.as_str()),
        _ => None,
    }
}

fn blob_hash(blob: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    blob.hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn encrypted_content_hashes(items: &[ResponseItem]) -> HashSet<u64> {
    items
        .iter()
        .filter_map(encrypted_blob)
        .map(blob_hash)
        .collect()
}

/// Removes reasoning and compaction items carrying one of `hashes`. A reasoning item cannot
/// be replayed without its encrypted content when responses are not stored, so the whole
/// item is dropped. Returns the number of removed items.
pub(crate) fn strip_encrypted_items(items: &mut Vec<ResponseItem>, hashes: &HashSet<u64>) -> usize {
    let before = items.len();
    items.retain(|item| encrypted_blob(item).is_none_or(|blob| !hashes.contains(&blob_hash(blob))));
    before - items.len()
}

#[cfg(test)]
#[path = "account_rotation_tests.rs"]
mod tests;
