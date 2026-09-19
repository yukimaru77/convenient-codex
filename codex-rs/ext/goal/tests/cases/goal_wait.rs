use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn goal_wait_needs_one_containing_response_from_any_turn() -> anyhow::Result<()> {
    let db = test_runtime().await?;
    let thread_id = test_thread_id()?;
    seed_thread_metadata(db.as_ref(), thread_id).await?;
    let harness = GoalExtensionHarness::new(db.clone(), thread_id).await?;
    for (index, trigger, message) in [
        (0, Some("goal"), "GOAL_WAIT"),
        (1, None, "External work pending: GOAL_WAIT"),
        (2, Some("monitor"), "`GOAL_WAIT`\nAwaiting results."),
    ] {
        db.thread_goals()
            .replace_thread_goal(
                thread_id,
                "consume external work",
                codex_state::ThreadGoalStatus::Active,
                None,
            )
            .await?;
        let turn = format!("turn-{index}");
        harness
            .start_turn_with_trigger(&turn, ModeKind::Default, &TokenUsage::default(), trigger)
            .await;
        harness
            .stop_turn_with_message(&turn, Some(message), /*active_monitor_count*/ 1)
            .await;
        assert_eq!(
            db.thread_goals()
                .get_thread_goal(thread_id)
                .await?
                .unwrap()
                .status,
            codex_state::ThreadGoalStatus::GoalWait
        );
    }
    // Reopening without a turn does not wake a persisted waiting goal.
    harness
        .goal_service
        .restore_thread_runtime_after_resume(thread_id)
        .await?;
    assert_eq!(
        db.thread_goals()
            .get_thread_goal(thread_id)
            .await?
            .unwrap()
            .status,
        codex_state::ThreadGoalStatus::GoalWait
    );
    harness
        .start_turn_with_trigger(
            "notification",
            ModeKind::Default,
            &TokenUsage::default(),
            Some("monitor"),
        )
        .await;
    assert_eq!(
        db.thread_goals()
            .get_thread_goal(thread_id)
            .await?
            .unwrap()
            .status,
        codex_state::ThreadGoalStatus::Active
    );
    harness
        .stop_turn_with_message(
            "notification",
            Some("Use the result"),
            /*active_monitor_count*/ 0,
        )
        .await;
    assert_eq!(
        db.thread_goals()
            .get_thread_goal(thread_id)
            .await?
            .unwrap()
            .status,
        codex_state::ThreadGoalStatus::Active
    );
    Ok(())
}

#[tokio::test]
async fn external_turn_wakes_only_goal_wait() -> anyhow::Result<()> {
    let db = test_runtime().await?;
    let thread_id = test_thread_id()?;
    seed_thread_metadata(db.as_ref(), thread_id).await?;
    let harness = GoalExtensionHarness::new(db.clone(), thread_id).await?;
    for status in [
        codex_state::ThreadGoalStatus::Paused,
        codex_state::ThreadGoalStatus::Blocked,
        codex_state::ThreadGoalStatus::UsageLimited,
        codex_state::ThreadGoalStatus::BudgetLimited,
        codex_state::ThreadGoalStatus::Complete,
        codex_state::ThreadGoalStatus::GoalWait,
    ] {
        db.thread_goals()
            .replace_thread_goal(thread_id, "preserve non-wait stops", status, None)
            .await?;
        harness.start_turn("user", &TokenUsage::default()).await;
        assert_eq!(
            db.thread_goals()
                .get_thread_goal(thread_id)
                .await?
                .unwrap()
                .status,
            if status == codex_state::ThreadGoalStatus::GoalWait {
                codex_state::ThreadGoalStatus::Active
            } else {
                status
            }
        );
        harness
            .stop_turn_with_message("user", Some("GOAL_WAIT"), /*active_monitor_count*/ 1)
            .await;
        assert_eq!(
            db.thread_goals()
                .get_thread_goal(thread_id)
                .await?
                .unwrap()
                .status,
            status
        );
    }
    Ok(())
}

#[tokio::test]
async fn wait_without_a_monitor_keeps_goal_active() -> anyhow::Result<()> {
    let db = test_runtime().await?;
    let thread_id = test_thread_id()?;
    seed_thread_metadata(db.as_ref(), thread_id).await?;
    let harness = GoalExtensionHarness::new(db.clone(), thread_id).await?;
    db.thread_goals()
        .replace_thread_goal(
            thread_id,
            "receive a real result",
            codex_state::ThreadGoalStatus::Active,
            None,
        )
        .await?;
    harness
        .start_turn("no-monitor", &TokenUsage::default())
        .await;
    harness
        .stop_turn_with_message(
            "no-monitor",
            Some("Awaiting: GOAL_WAIT"),
            /*active_monitor_count*/ 0,
        )
        .await;
    assert_eq!(
        db.thread_goals()
            .get_thread_goal(thread_id)
            .await?
            .unwrap()
            .status,
        codex_state::ThreadGoalStatus::Active
    );
    Ok(())
}
