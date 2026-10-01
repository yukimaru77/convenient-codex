use std::path::PathBuf;
use std::sync::Arc;

use codex_config::types::AccountRotationConfig;
use codex_protocol::protocol::RateLimitSnapshot;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

use crate::session::tests::make_session_and_context;

fn rate_limits() -> RateLimitSnapshot {
    RateLimitSnapshot {
        limit_id: Some("codex".to_string()),
        limit_name: None,
        normal_model_slug: None,
        primary: None,
        secondary: None,
        credits: None,
        individual_limit: None,
        spend_control_reached: None,
        plan_type: None,
        rate_limit_reached_type: None,
    }
}

#[tokio::test]
async fn rotation_hook_is_a_no_op_without_config() {
    let (session, _turn_context) = make_session_and_context().await;
    assert_eq!(session.get_config().await.account_rotation, None);
    session.state.lock().await.set_rate_limits(rate_limits());
    let home_before = session.services.auth_manager.auth_home();

    assert_eq!(session.maybe_rotate_account("turn-1").await, None);

    let state = session.state.lock().await;
    assert!(state.latest_rate_limits.is_some());
    assert_eq!(state.account_rotation_home, None);
    assert_eq!(session.services.auth_manager.auth_home(), home_before);
}

#[tokio::test]
async fn rate_limits_from_another_account_are_reset() {
    let (session, _turn_context) = make_session_and_context().await;
    let accounts = tempfile::tempdir().expect("tempdir");
    {
        let mut state = session.state.lock().await;
        let mut config = (*state.session_configuration.original_config_do_not_use).clone();
        config.account_rotation = Some(AccountRotationConfig {
            accounts_dir: AbsolutePathBuf::from_absolute_path(accounts.path()).expect("absolute"),
            reserve_percent: 10,
            usage_cache_seconds: 60,
        });
        state.session_configuration.original_config_do_not_use = Arc::new(config);
        state.set_rate_limits(rate_limits());
        state.account_rotation_home = Some(PathBuf::from("/previous/account"));
    }

    // API-key auth never rotates, but limits observed under another home are dropped.
    assert_eq!(session.maybe_rotate_account("turn-1").await, None);

    let state = session.state.lock().await;
    assert_eq!(state.latest_rate_limits, None);
    assert_eq!(
        state.account_rotation_home,
        Some(session.services.auth_manager.auth_home())
    );
}

#[tokio::test]
async fn reset_rate_limits_clears_the_merged_snapshot() {
    let (session, _turn_context) = make_session_and_context().await;
    let mut state = session.state.lock().await;
    state.set_rate_limits(rate_limits());
    state.reset_rate_limits();
    assert_eq!(state.latest_rate_limits, None);
}

#[tokio::test]
async fn rejected_encrypted_content_is_stripped_from_later_prompts() {
    use codex_protocol::models::ResponseItem;

    let (session, _turn_context) = make_session_and_context().await;
    let reasoning = |blob: &str| ResponseItem::Reasoning {
        id: None,
        summary: Vec::new(),
        content: None,
        encrypted_content: Some(blob.to_string()),
        internal_chat_message_metadata_passthrough: None,
    };
    let mut prompt = vec![reasoning("from-old-account")];
    session.strip_rejected_encrypted_content(&mut prompt).await;
    assert_eq!(prompt.len(), 1);

    assert_eq!(session.reject_encrypted_content(&prompt).await, 1);
    let mut prompt = vec![reasoning("from-old-account"), reasoning("from-new-account")];
    session.strip_rejected_encrypted_content(&mut prompt).await;
    assert_eq!(prompt, vec![reasoning("from-new-account")]);
}
