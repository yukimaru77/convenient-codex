//! Session hooks for turn-boundary account rotation; see `crate::account_rotation`.

use chrono::DateTime;
use chrono::Utc;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::WarningEvent;

use super::session::Session;
use crate::account_rotation::AccountSwitch;
use crate::account_rotation::encrypted_content_hashes;
use crate::account_rotation::mark_exhausted;
use crate::account_rotation::maybe_rotate;
use crate::account_rotation::rotation_applies_to;
use crate::account_rotation::strip_encrypted_items;
use std::sync::Arc;
use std::sync::Weak;
use tracing::info;

/// Runs turn-end accounting on every exit path, including cancellation and errors.
pub(crate) struct TurnEndGuard {
    session: Weak<Session>,
    sub_id: String,
}

impl Drop for TurnEndGuard {
    fn drop(&mut self) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let sub_id = self.sub_id.clone();
        tokio::spawn(async move {
            session.finish_turn_account_rotation(&sub_id).await;
        });
    }
}

impl Session {
    pub(crate) fn turn_end_guard(self: &Arc<Self>, sub_id: &str) -> TurnEndGuard {
        TurnEndGuard {
            session: Arc::downgrade(self),
            sub_id: sub_id.to_string(),
        }
    }
}

impl Session {
    /// Turn-boundary hook. A no-op unless `[account_rotation]` is configured.
    pub(crate) async fn maybe_rotate_account(&self, sub_id: &str) -> Option<AccountSwitch> {
        let config = self.get_config().await;
        let rotation = config.account_rotation.as_ref()?;
        let auth_manager = &self.services.auth_manager;
        let latest = {
            let mut state = self.state.lock().await;
            if !rotation_applies_to(&state.session_configuration.session_source) {
                return None;
            }
            // Another session may have switched accounts since these limits were observed.
            let home = auth_manager.auth_home();
            if state.account_rotation_home.as_ref() != Some(&home) {
                state.reset_rate_limits();
                state.account_rotation_home = Some(home);
            }
            state.latest_rate_limits.clone()
        };
        info!(
            sub_id = %sub_id,
            account = %auth_manager.auth_home().display(),
            "account rotation: turn start"
        );
        let switch = maybe_rotate(
            rotation,
            auth_manager,
            latest.as_ref(),
            &config.chatgpt_base_url,
        )
        .await?;
        {
            let mut state = self.state.lock().await;
            state.reset_rate_limits();
            state.account_rotation_home = Some(auth_manager.auth_home());
        }
        self.send_event_raw(Event {
            id: sub_id.to_string(),
            msg: EventMsg::Warning(WarningEvent {
                message: switch.notice(),
            }),
        })
        .await;
        Some(switch)
    }

    /// Checks the completed turn's latest limits and switches while no more requests from
    /// that turn are pending. The next turn then starts with the selected account.
    pub(crate) async fn finish_turn_account_rotation(&self, sub_id: &str) {
        let config = self.get_config().await;
        let Some(rotation) = config.account_rotation.as_ref() else {
            return;
        };
        let latest = self.state.lock().await.latest_rate_limits.clone();
        info!(
            sub_id = %sub_id,
            account = %self.services.auth_manager.auth_home().display(),
            latest_snapshot = latest.is_some(),
            "account rotation: turn end"
        );
        if let Some(switch) = maybe_rotate(
            rotation,
            &self.services.auth_manager,
            latest.as_ref(),
            &config.chatgpt_base_url,
        )
        .await
        {
            self.state.lock().await.reset_rate_limits();
            self.send_event_raw(Event {
                id: sub_id.to_string(),
                msg: EventMsg::Warning(WarningEvent {
                    message: switch.notice(),
                }),
            })
            .await;
        }
    }

    /// Records that the current account hit its usage limit so the next turn rotates.
    pub(crate) async fn note_usage_limit_reached(&self, resets_at: Option<DateTime<Utc>>) {
        if self.get_config().await.account_rotation.is_some() {
            mark_exhausted(&self.services.auth_manager.auth_home(), resets_at);
        }
    }

    /// Remembers the encrypted blobs in `input` as rejected and reports how many were found.
    pub(crate) async fn reject_encrypted_content(&self, input: &[ResponseItem]) -> usize {
        let hashes = encrypted_content_hashes(input);
        let count = hashes.len();
        self.state
            .lock()
            .await
            .rejected_encrypted_content
            .extend(hashes);
        count
    }

    /// Drops items whose encrypted blobs the current account previously rejected.
    pub(crate) async fn strip_rejected_encrypted_content(&self, input: &mut Vec<ResponseItem>) {
        let state = self.state.lock().await;
        if !state.rejected_encrypted_content.is_empty() {
            strip_encrypted_items(input, &state.rejected_encrypted_content);
        }
    }
}

#[cfg(test)]
#[path = "account_rotation_tests.rs"]
mod tests;
