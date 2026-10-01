use anyhow::Context;
use anyhow::Result;
use base64::Engine;
use chrono::Duration;
use chrono::Utc;
use codex_config::types::AuthCredentialsStoreMode;
use codex_login::AuthDotJson;
use codex_login::AuthKeyringBackendKind;
use codex_login::AuthManager;
use codex_login::CLIENT_ID_OVERRIDE_ENV_VAR;
use codex_login::REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR;
use codex_login::load_auth_dot_json;
use codex_login::save_auth;
use codex_login::token_data::TokenData;
use codex_login::token_data::parse_chatgpt_jwt_claims;
use codex_protocol::auth::AuthMode;
use core_test_support::skip_if_no_network;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

fn id_token(user_id: &str, account_id: &str) -> String {
    let b64 = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let header = b64(br#"{"alg":"none","typ":"JWT"}"#);
    let payload = json!({
        "email": format!("{user_id}@example.com"),
        "https://api.openai.com/auth": {
            "chatgpt_user_id": user_id,
            "chatgpt_account_id": account_id,
        },
    });
    let payload = b64(payload.to_string().as_bytes());
    format!("{header}.{payload}.{}", b64(b"sig"))
}

fn write_account(home: &Path, user_id: &str, account_id: &str, token: &str) -> Result<()> {
    let tokens = TokenData {
        id_token: parse_chatgpt_jwt_claims(&id_token(user_id, account_id))?,
        access_token: format!("{token}-access"),
        refresh_token: format!("{token}-refresh"),
        account_id: Some(account_id.to_string()),
    };
    let auth = AuthDotJson {
        auth_mode: Some(AuthMode::Chatgpt),
        openai_api_key: None,
        tokens: Some(tokens),
        last_refresh: Some(Utc::now() - Duration::hours(1)),
        agent_identity: None,
        personal_access_token: None,
        bedrock_api_key: None,
        bedrock_access_keys: None,
    };
    save_auth(
        home,
        &auth,
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )?;
    Ok(())
}

fn stored_tokens(home: &Path) -> Result<TokenData> {
    load_auth_dot_json(
        home,
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )?
    .context("auth.json should exist")?
    .tokens
    .context("tokens should exist")
}

async fn manager_for(home: &Path) -> Arc<AuthManager> {
    AuthManager::shared(
        home.to_path_buf(),
        /*enable_codex_api_key_env*/ false,
        AuthCredentialsStoreMode::File,
        /*forced_chatgpt_workspace_id*/ None,
        /*chatgpt_base_url*/ None,
        AuthKeyringBackendKind::default(),
        codex_login::test_support::transport_default_auth_route_config(),
    )
    .await
}

#[tokio::test]
async fn switch_home_reports_owner_changes_and_bumps_owner_generation() -> Result<()> {
    let first = TempDir::new()?;
    let second = TempDir::new()?;
    let second_same_owner = TempDir::new()?;
    write_account(first.path(), "user-a", "account-a", "a")?;
    write_account(second.path(), "user-b", "account-b", "b")?;
    write_account(second_same_owner.path(), "user-b", "account-b", "b2")?;

    let manager = manager_for(first.path()).await;
    let changes = manager.auth_change_state_receiver();
    let initial_owner_generation = changes.borrow().owner_generation;
    assert_eq!(
        manager.auth_cached().and_then(|auth| auth.get_account_id()),
        Some("account-a".to_string())
    );

    assert!(manager.switch_home(second.path().to_path_buf()).await);
    assert_eq!(manager.auth_home(), second.path().to_path_buf());
    assert_eq!(
        manager.auth_cached().and_then(|auth| auth.get_account_id()),
        Some("account-b".to_string())
    );
    assert_eq!(
        changes.borrow().owner_generation,
        initial_owner_generation + 1
    );

    // Same owner with refreshed credentials is a credential change, not an owner change.
    assert!(
        !manager
            .switch_home(second_same_owner.path().to_path_buf())
            .await
    );
    assert_eq!(
        changes.borrow().owner_generation,
        initial_owner_generation + 1
    );
    Ok(())
}

#[serial_test::serial(auth_env)]
#[tokio::test]
async fn refresh_after_switch_home_persists_into_the_new_home() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "new-access-token",
            "refresh_token": "new-refresh-token"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let _client_id_guard = EnvGuard::set(CLIENT_ID_OVERRIDE_ENV_VAR, "staging-client".into());
    let _endpoint_guard = EnvGuard::set(
        REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR,
        format!("{}/oauth/token", server.uri()),
    );

    let first = TempDir::new()?;
    let second = TempDir::new()?;
    write_account(first.path(), "user-a", "account-a", "a")?;
    write_account(second.path(), "user-b", "account-b", "b")?;
    let first_before = stored_tokens(first.path())?;

    let manager = manager_for(first.path()).await;
    assert!(manager.switch_home(second.path().to_path_buf()).await);
    manager
        .refresh_token_from_authority()
        .await
        .context("refresh should succeed")?;

    let second_after = stored_tokens(second.path())?;
    assert_eq!(second_after.access_token, "new-access-token");
    assert_eq!(second_after.refresh_token, "new-refresh-token");
    assert_eq!(second_after.account_id.as_deref(), Some("account-b"));
    assert_eq!(stored_tokens(first.path())?, first_before);
    server.verify().await;
    Ok(())
}

struct EnvGuard {
    key: &'static str,
    original: Option<OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: String) -> Self {
        let original = std::env::var_os(key);
        // SAFETY: these tests execute serially, so updating the process environment is safe.
        unsafe {
            std::env::set_var(key, &value);
        }
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: the guard restores the original environment value before other tests run.
        unsafe {
            match &self.original {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}
