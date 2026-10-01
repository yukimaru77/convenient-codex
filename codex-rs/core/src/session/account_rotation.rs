//! Session hooks for turn-boundary account rotation; see `crate::account_rotation`.

use chrono::DateTime;
use chrono::Utc;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::WarningEvent;

use super::session::Session;
use crate::account_rotation::AccountSwitch;
use crate::account_rotation::mark_exhausted;
use crate::account_rotation::maybe_rotate;
use crate::account_rotation::rotation_applies_to;

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

    /// Records that the current account hit its usage limit so the next turn rotates.
    pub(crate) async fn note_usage_limit_reached(&self, resets_at: Option<DateTime<Utc>>) {
        if self.get_config().await.account_rotation.is_some() {
            mark_exhausted(&self.services.auth_manager.auth_home(), resets_at);
        }
    }
}

#[cfg(test)]
#[path = "account_rotation_tests.rs"]
mod tests;
