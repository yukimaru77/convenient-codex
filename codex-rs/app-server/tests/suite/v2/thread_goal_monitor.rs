#![cfg(not(windows))]

use std::time::Duration;

use anyhow::Context;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ThreadBackgroundTerminalsListResponse;
use codex_app_server_protocol::ThreadGoalSetResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use core_test_support::skip_if_remote;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use tokio::time::timeout;

#[path = "thread_goal_monitor_wait.rs"]
mod wait_cases;

#[tokio::test]
async fn active_goal_continues_with_summary_monitor() -> anyhow::Result<()> {
    assert_monitor_does_not_gate_goal("monitor").await
}

#[tokio::test]
async fn active_goal_continues_with_realtime_monitor() -> anyhow::Result<()> {
    assert_monitor_does_not_gate_goal("monitor_realtime").await
}

async fn assert_monitor_does_not_gate_goal(tool_name: &str) -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_remote!(Ok(()), "test command runs on the host");
    let server = responses::start_mock_server().await;
    let home = TempDir::new()?;
    responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "start-monitor",
                    tool_name,
                    &json!({
                        "action":"start", "description":"independent work", "command":"sleep 120",
                    })
                    .to_string(),
                ),
                responses::ev_completed("start"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("launched", "Working independently."),
                responses::ev_completed("launched"),
            ]),
        ],
    )
    .await;
    MockResponsesConfig::new(&server.uri())
        .with_model("gpt-5.4")
        .enable_feature(Feature::Goals)
        .enable_feature(Feature::Monitor)
        .write(home.path())?;
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .without_managed_config()
        .build_initialized()
        .await?;
    let id = app
        .send_thread_start_request_with_auto_env(ThreadStartParams::default())
        .await?;
    let started: ThreadStartResponse = app.read_response(id).await?;
    let id = app
        .send_turn_start_request(TurnStartParams {
            thread_id: started.thread.id.clone(),
            input: vec![UserInput::Text {
                text: "Launch background work.".into(),
                text_elements: vec![],
            }],
            ..Default::default()
        })
        .await?;
    let _: TurnStartResponse = app.read_response(id).await?;
    timeout(
        Duration::from_secs(20),
        app.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    let followup = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_function_call("done", "update_goal", r#"{"status":"complete"}"#),
                responses::ev_completed("complete"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("done", "Independent task done."),
                responses::ev_completed("done"),
            ]),
        ],
    )
    .await;
    let id = app
        .send_raw_request(
            "thread/goal/set",
            Some(json!({"threadId":started.thread.id, "objective":"Do the independent work"})),
        )
        .await?;
    let _: ThreadGoalSetResponse = app.read_response(id).await?;
    timeout(
        Duration::from_secs(20),
        app.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    assert_eq!(followup.requests().len(), 2);
    let metadata: serde_json::Value = serde_json::from_str(
        &followup.requests()[0]
            .header("x-codex-turn-metadata")
            .context("expected goal continuation metadata")?,
    )?;
    assert_eq!(metadata["turn_trigger"], "goal");
    let id = app
        .send_raw_request(
            "thread/backgroundTerminals/list",
            Some(json!({"threadId": started.thread.id})),
        )
        .await?;
    let remaining: ThreadBackgroundTerminalsListResponse = app.read_response(id).await?;
    assert_eq!(
        remaining.monitors.len(),
        1,
        "independent continuation leaves the job running"
    );
    Ok(())
}
