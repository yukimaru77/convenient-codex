use super::*;
use codex_app_server_protocol::ThreadGoalStatus;
use codex_app_server_protocol::ThreadGoalUpdatedNotification;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn goal_wait_wakes_on_user_turn() -> anyhow::Result<()> {
    exercise_goal_wait("user").await
}

#[tokio::test]
async fn goal_wait_wakes_on_actual_monitor_exit() -> anyhow::Result<()> {
    exercise_goal_wait("monitor").await
}

async fn exercise_goal_wait(wake: &str) -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_remote!(Ok(()), "test process runs on host");
    let server = responses::start_mock_server().await;
    let home = TempDir::new()?;
    let pipe = home.path().join("external-result.fifo");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&pipe)
            .status()?
            .success()
    );
    let mut sequence = Vec::new();
    sequence.extend([
            responses::sse(vec![responses::ev_function_call("start", "monitor", &json!({
                "action": "start", "description": "finite external result",
                "command": format!("read -r result < '{}'; printf '%s\\n' \"$result\"", pipe.display()),
            }).to_string()), responses::ev_completed("monitor")]),
            responses::sse(vec![responses::ev_assistant_message("launched", "External computation started."), responses::ev_completed("launched")]),
    ]);
    sequence.push(responses::sse(vec![
        responses::ev_assistant_message("wait", "Waiting: `GOAL_WAIT`\nExternal result pending."),
        responses::ev_completed("wait"),
    ]));
    sequence.extend([
        responses::sse(vec![
            responses::ev_assistant_message("awake", "Using the external result."),
            responses::ev_completed("awake"),
        ]),
        responses::sse(vec![
            responses::ev_function_call("complete", "update_goal", r#"{"status":"complete"}"#),
            responses::ev_completed("complete"),
        ]),
        responses::sse(vec![
            responses::ev_assistant_message("done", "Done."),
            responses::ev_completed("done"),
        ]),
    ]);
    let requests = responses::mount_sse_sequence(&server, sequence).await;
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
    {
        let id = app
            .send_turn_start_request(TurnStartParams {
                thread_id: started.thread.id.clone(),
                input: vec![UserInput::Text {
                    text: "Start external work".into(),
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
    }
    let id = app
        .send_raw_request(
            "thread/goal/set",
            Some(json!({"threadId": started.thread.id, "objective": "Use the external result"})),
        )
        .await?;
    let _: ThreadGoalSetResponse = app.read_response(id).await?;
    let waiting_turn = timeout(Duration::from_secs(30), async {
        loop {
            let note: ThreadGoalUpdatedNotification =
                app.read_notification("thread/goal/updated").await?;
            if note.goal.status == ThreadGoalStatus::GoalWait {
                return Ok::<_, anyhow::Error>(note.turn_id.unwrap());
            }
        }
    })
    .await??;
    // The state update is emitted before the corresponding completed-turn event.
    timeout(
        Duration::from_secs(20),
        app.read_stream_until_matching_notification("waiting turn completed", |note| {
            note.method == "turn/completed"
                && note
                    .params
                    .as_ref()
                    .is_some_and(|params| params["turn"]["id"] == waiting_turn)
        }),
    )
    .await??;
    let before = 3;
    assert_eq!(requests.requests().len(), before);
    if wake == "monitor" {
        // A finite real process exits, delivering the tool's actual notification.
        tokio::task::spawn_blocking(move || std::fs::write(pipe, "READY\n")).await??;
    } else {
        let id = app
            .send_turn_start_request(TurnStartParams {
                thread_id: started.thread.id.clone(),
                input: vec![UserInput::Text {
                    text: "The result is ready".into(),
                    text_elements: vec![],
                }],
                ..Default::default()
            })
            .await?;
        let _: TurnStartResponse = app.read_response(id).await?;
    }
    let completed_turn = timeout(Duration::from_secs(30), async {
        let mut saw_active = false;
        loop {
            let note: ThreadGoalUpdatedNotification =
                app.read_notification("thread/goal/updated").await?;
            saw_active |= note.goal.status == ThreadGoalStatus::Active;
            if note.goal.status == ThreadGoalStatus::Complete {
                assert!(saw_active);
                return Ok::<_, anyhow::Error>(note.turn_id.unwrap());
            }
        }
    })
    .await??;
    timeout(
        Duration::from_secs(20),
        app.read_stream_until_matching_notification("goal completion turn finished", |note| {
            note.method == "turn/completed"
                && note
                    .params
                    .as_ref()
                    .is_some_and(|params| params["turn"]["id"] == completed_turn)
        }),
    )
    .await??;
    assert_eq!(requests.requests().len(), before + 3);
    Ok(())
}

#[tokio::test]
async fn goal_wait_without_monitor_continues_with_explanation() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    let home = TempDir::new()?;
    let requests = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_assistant_message("wait", "Please wait: GOAL_WAIT"),
                responses::ev_completed("wait"),
            ]),
            responses::sse(vec![
                responses::ev_function_call("complete", "update_goal", r#"{"status":"complete"}"#),
                responses::ev_completed("complete"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("done", "Recovered from unsupported wait."),
                responses::ev_completed("done"),
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
    let id = app.send_raw_request("thread/goal/set", Some(json!({
        "threadId": started.thread.id, "objective": "Continue useful work without a Monitor",
    }))).await?;
    let _: ThreadGoalSetResponse = app.read_response(id).await?;
    let completed_turn = timeout(Duration::from_secs(30), async {
        loop {
            let note: ThreadGoalUpdatedNotification =
                app.read_notification("thread/goal/updated").await?;
            assert_ne!(note.goal.status, ThreadGoalStatus::GoalWait);
            if note.goal.status == ThreadGoalStatus::Complete {
                return Ok::<_, anyhow::Error>(note.turn_id.unwrap());
            }
        }
    })
    .await??;
    timeout(
        Duration::from_secs(20),
        app.read_stream_until_matching_notification("recovery completed", |note| {
            note.method == "turn/completed"
                && note
                    .params
                    .as_ref()
                    .is_some_and(|p| p["turn"]["id"] == completed_turn)
        }),
    )
    .await??;
    let seen = requests.requests();
    assert_eq!(seen.len(), 3);
    assert!(
        seen[1].body_json()["input"]
            .to_string()
            .contains("GOAL_WAIT was not accepted: there are no active Monitors in this session.")
    );
    let metadata: serde_json::Value =
        serde_json::from_str(&seen[1].header("x-codex-turn-metadata").unwrap())?;
    assert_eq!(metadata["turn_trigger"], "goal");
    Ok(())
}
