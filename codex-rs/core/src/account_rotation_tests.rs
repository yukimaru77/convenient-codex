use super::*;
use codex_config::types::AuthCredentialsStoreMode;
use codex_login::AuthKeyringBackendKind;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;

fn usage(remaining: f64, resets_at: i64) -> Option<AccountUsage> {
    Some(AccountUsage {
        weekly_remaining_percent: remaining,
        weekly_resets_at: Some(resets_at),
        blocked: false,
    })
}

fn candidate(name: &str, usage: Option<AccountUsage>) -> AccountCandidate {
    AccountCandidate {
        name: name.to_string(),
        dir: PathBuf::from(format!("/accounts/{name}")),
        usage,
    }
}

fn selected<'a>(candidates: &'a [AccountCandidate], current: Option<&str>) -> Option<&'a str> {
    let current = current.map(|name| PathBuf::from(format!("/accounts/{name}")));
    select_account(candidates, current.as_deref(), /*reserve_percent*/ 10)
        .map(|candidate| candidate.name.as_str())
}

fn window(used_percent: f64, window_minutes: i64, resets_at: i64) -> RateLimitWindow {
    RateLimitWindow {
        used_percent,
        window_minutes: Some(window_minutes),
        resets_at: Some(resets_at),
    }
}

fn snapshot(
    primary: Option<RateLimitWindow>,
    secondary: Option<RateLimitWindow>,
) -> RateLimitSnapshot {
    RateLimitSnapshot {
        limit_id: Some("codex".to_string()),
        limit_name: None,
        normal_model_slug: None,
        primary,
        secondary,
        credits: None,
        individual_limit: None,
        spend_control_reached: None,
        plan_type: None,
        rate_limit_reached_type: None,
    }
}

#[test]
fn usage_uses_the_longest_window_and_blocks_on_any_full_window() {
    let usage = AccountUsage::from_snapshot(&snapshot(
        Some(window(30.0, 300, 10)),
        Some(window(60.0, 10_080, 20)),
    ))
    .expect("usage");
    assert_eq!(
        usage,
        AccountUsage {
            weekly_remaining_percent: 40.0,
            weekly_resets_at: Some(20),
            blocked: false,
        }
    );

    let five_hour_full = AccountUsage::from_snapshot(&snapshot(
        Some(window(100.0, 300, 10)),
        Some(window(5.0, 10_080, 20)),
    ))
    .expect("usage");
    assert!(five_hour_full.blocked);
    assert!(!five_hour_full.is_usable(/*reserve_percent*/ 10));

    assert_eq!(AccountUsage::from_snapshot(&snapshot(None, None)), None);
}

#[test]
fn usable_requires_weekly_remaining_above_reserve() {
    assert!(usage(10.5, 0).unwrap().is_usable(10));
    assert!(!usage(10.0, 0).unwrap().is_usable(10));
    assert!(!usage(3.0, 0).unwrap().is_usable(10));
}

#[test]
fn selection_fills_the_account_resetting_first() {
    let candidates = [
        candidate("a", usage(80.0, 300)),
        candidate("b", usage(50.0, 100)),
        candidate("c", usage(90.0, 200)),
    ];
    assert_eq!(selected(&candidates, None), Some("b"));
}

#[test]
fn selection_breaks_reset_ties_by_name() {
    let candidates = [
        candidate("zeta", usage(80.0, 100)),
        candidate("alpha", usage(50.0, 100)),
    ];
    assert_eq!(selected(&candidates, None), Some("alpha"));
}

#[test]
fn selection_skips_accounts_at_or_below_reserve_and_unknown_usage() {
    let candidates = [
        candidate("a", usage(5.0, 100)),
        candidate("b", None),
        candidate("c", usage(40.0, 300)),
    ];
    assert_eq!(selected(&candidates, Some("a")), Some("c"));
}

#[test]
fn selection_keeps_a_usable_current_account() {
    let candidates = [
        candidate("a", usage(80.0, 100)),
        candidate("b", usage(50.0, 500)),
    ];
    assert_eq!(selected(&candidates, Some("b")), Some("b"));
}

#[test]
fn selection_returns_none_when_nothing_is_usable() {
    let candidates = [
        candidate("a", usage(2.0, 100)),
        candidate(
            "b",
            Some(AccountUsage {
                weekly_remaining_percent: 70.0,
                weekly_resets_at: Some(50),
                blocked: true,
            }),
        ),
    ];
    assert_eq!(selected(&candidates, Some("a")), None);
}

#[test]
fn list_accounts_skips_disabled_and_unreadable_accounts() {
    let accounts = TempDir::new().expect("tempdir");
    for name in ["b", "a", "disabled-one", "broken", "empty"] {
        std::fs::create_dir(accounts.path().join(name)).expect("mkdir");
    }
    std::fs::write(accounts.path().join("a/auth.json"), "{}").expect("write");
    std::fs::write(accounts.path().join("b/auth.json"), "{}").expect("write");
    std::fs::write(accounts.path().join("disabled-one/auth.json"), "{}").expect("write");
    std::fs::write(
        accounts.path().join("disabled-one").join(DISABLED_MARKER),
        "",
    )
    .expect("write");
    std::fs::write(accounts.path().join("broken/auth.json"), "not json").expect("write");
    std::fs::write(accounts.path().join("stray-file"), "{}").expect("write");

    let names: Vec<String> = list_accounts(accounts.path())
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn switch_notice_is_one_line() {
    let switch = AccountSwitch {
        from: "alpha".to_string(),
        to: "beta".to_string(),
        remaining_percent: 72.4,
    };
    assert_eq!(
        switch.notice(),
        "account: switched alpha → beta (remaining 72%)"
    );
}

#[test]
fn rotation_only_applies_to_sessions_that_own_their_turns() {
    assert!(rotation_applies_to(&SessionSource::Cli));
    assert!(rotation_applies_to(&SessionSource::Exec));
    assert!(!rotation_applies_to(&SessionSource::SubAgent(
        codex_protocol::protocol::SubAgentSource::Review
    )));
}

fn write_account(accounts: &Path, name: &str) -> PathBuf {
    use base64::Engine;
    let dir = accounts.join(name);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let b64 = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let claims = json!({
        "https://api.openai.com/auth": {
            "chatgpt_user_id": format!("user-{name}"),
            "chatgpt_account_id": format!("account-{name}"),
        },
    });
    let id_token = format!(
        "{}.{}.{}",
        b64(br#"{"alg":"none","typ":"JWT"}"#),
        b64(claims.to_string().as_bytes()),
        b64(b"sig")
    );
    let auth = json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {
            "id_token": id_token,
            "access_token": format!("token-{name}"),
            "refresh_token": format!("refresh-{name}"),
            "account_id": format!("account-{name}"),
        },
        "last_refresh": Utc::now(),
    });
    std::fs::write(dir.join(AUTH_FILE), auth.to_string()).expect("write auth");
    dir
}

async fn mount_usage(server: &MockServer, name: &str, weekly_used: i32, weekly_reset_at: i32) {
    Mock::given(method("GET"))
        .and(path("/api/codex/usage"))
        .and(header(
            "authorization",
            format!("Bearer token-{name}").as_str(),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "plan_type": "pro",
            "rate_limit": {
                "allowed": true,
                "limit_reached": false,
                "primary_window": {
                    "used_percent": 5,
                    "limit_window_seconds": 18_000,
                    "reset_after_seconds": 60,
                    "reset_at": 1_900_000_000,
                },
                "secondary_window": {
                    "used_percent": weekly_used,
                    "limit_window_seconds": 604_800,
                    "reset_after_seconds": 600,
                    "reset_at": weekly_reset_at,
                },
            },
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn rotation_switches_to_the_usable_account_resetting_first() {
    let server = MockServer::start().await;
    let accounts = TempDir::new().expect("tempdir");
    let current = write_account(accounts.path(), "a");
    write_account(accounts.path(), "b");
    write_account(accounts.path(), "c");
    write_account(accounts.path(), "d");
    mount_usage(&server, "b", 40, 1_900_000_300).await;
    mount_usage(&server, "c", 20, 1_900_000_100).await;
    // Resets first but is inside the reserve.
    mount_usage(&server, "d", 95, 1_900_000_050).await;

    let auth_manager = AuthManager::shared(
        current.clone(),
        /*enable_codex_api_key_env*/ false,
        AuthCredentialsStoreMode::File,
        /*forced_chatgpt_workspace_id*/ None,
        /*chatgpt_base_url*/ None,
        AuthKeyringBackendKind::default(),
        codex_login::test_support::transport_default_auth_route_config(),
    )
    .await;
    let config = AccountRotationConfig {
        accounts_dir: codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(
            accounts.path(),
        )
        .expect("absolute"),
        reserve_percent: 10,
        usage_cache_seconds: 60,
    };
    let base_url = server.uri();

    // A usable current account is kept without polling anyone.
    let healthy = snapshot(None, Some(window(50.0, 10_080, 1_900_000_000)));
    assert_eq!(
        maybe_rotate(&config, &auth_manager, Some(&healthy), &base_url).await,
        None
    );
    assert_eq!(auth_manager.auth_home(), current);

    let depleted = snapshot(None, Some(window(92.0, 10_080, 1_900_000_000)));
    let switch = maybe_rotate(&config, &auth_manager, Some(&depleted), &base_url)
        .await
        .expect("rotation should switch");
    assert_eq!(
        switch,
        AccountSwitch {
            from: "a".to_string(),
            to: "c".to_string(),
            remaining_percent: 80.0,
        }
    );
    assert_eq!(auth_manager.auth_home(), accounts.path().join("c"));
    assert_eq!(
        auth_manager
            .auth_cached()
            .and_then(|auth| auth.get_account_id()),
        Some("account-c".to_string())
    );

    // A usage-limit error excludes the new account even without fresh rate limits.
    mark_exhausted(&accounts.path().join("c"), /*resets_at*/ None);
    let switch = maybe_rotate(&config, &auth_manager, None, &base_url)
        .await
        .expect("rotation should switch again");
    assert_eq!(switch.to, "b");
}

#[tokio::test]
async fn rotation_keeps_the_current_account_when_nothing_is_usable() {
    let server = MockServer::start().await;
    let accounts = TempDir::new().expect("tempdir");
    let current = write_account(accounts.path(), "a");
    write_account(accounts.path(), "b");
    mount_usage(&server, "b", 99, 1_900_000_300).await;
    let auth_manager = AuthManager::shared(
        current.clone(),
        /*enable_codex_api_key_env*/ false,
        AuthCredentialsStoreMode::File,
        /*forced_chatgpt_workspace_id*/ None,
        /*chatgpt_base_url*/ None,
        AuthKeyringBackendKind::default(),
        codex_login::test_support::transport_default_auth_route_config(),
    )
    .await;
    let config = AccountRotationConfig {
        accounts_dir: codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(
            accounts.path(),
        )
        .expect("absolute"),
        reserve_percent: 10,
        usage_cache_seconds: 60,
    };
    let depleted = snapshot(None, Some(window(100.0, 10_080, 1_900_000_000)));
    assert_eq!(
        maybe_rotate(&config, &auth_manager, Some(&depleted), &server.uri()).await,
        None
    );
    assert_eq!(auth_manager.auth_home(), current);
}
