use super::*;

impl GoalRuntimeHandle {
    pub(crate) async fn wake_waiting_goal_for_turn(
        &self,
        turn_id: &str,
        trigger: Option<&str>,
    ) -> Result<(), String> {
        if trigger == Some("goal") {
            // continue_if_idle already holds the state permit while starting
            // this turn. Do not try to acquire that same permit recursively.
            return Ok(());
        }
        // An external turn already provides the requested wake-up opportunity.
        self.inner
            .wait_without_monitor
            .store(false, Ordering::Relaxed);
        let _permit = self.goal_state_permit().await?;
        let Some(goal) = self
            .inner
            .state_dbs
            .thread_goals()
            .get_thread_goal(self.thread_id())
            .await
            .map_err(|err| err.to_string())?
        else {
            return Ok(());
        };
        if goal.status != codex_state::ThreadGoalStatus::GoalWait {
            return Ok(());
        }
        let Some(updated) = self
            .inner
            .state_dbs
            .thread_goals()
            .update_thread_goal(
                self.thread_id(),
                codex_state::GoalUpdate {
                    objective: None,
                    status: Some(codex_state::ThreadGoalStatus::Active),
                    token_budget: None,
                    expected_goal_id: Some(goal.goal_id),
                },
            )
            .await
            .map_err(|err| err.to_string())?
        else {
            return Ok(());
        };
        self.inner
            .metrics
            .record_resumed_if_status_changed(Some(goal.status), updated.status);
        self.inner.analytics.status_changed(
            &updated,
            Some(goal.status),
            GoalEventAttribution::Turn(turn_id),
        );
        self.inner.event_emitter.thread_goal_updated(
            format!("{turn_id}:goal-wake"),
            Some(turn_id.to_string()),
            protocol_goal_from_state(updated),
        );
        // Do not schedule a second turn: the external turn is already starting.
        Ok(())
    }

    pub(crate) async fn record_goal_wait_at_turn_stop(
        &self,
        turn_id: &str,
        last_message: Option<&str>,
        active_monitor_count: usize,
    ) -> Result<(), String> {
        if !last_message.is_some_and(|message| message.contains("GOAL_WAIT")) {
            return Ok(());
        }
        let _permit = self.goal_state_permit().await?;
        // The host's last nonempty assistant message can be commentary when
        // the final is empty. Only a recorded final response may request wait.
        if !self.inner.accounting_state.goal_wait_requested(turn_id) {
            return Ok(());
        }
        let Some(goal) = self
            .inner
            .state_dbs
            .thread_goals()
            .get_thread_goal(self.thread_id())
            .await
            .map_err(|err| err.to_string())?
        else {
            return Ok(());
        };
        if goal.status != codex_state::ThreadGoalStatus::Active {
            return Ok(());
        }
        if active_monitor_count == 0 {
            // Keep the Goal active. Its ordinary next continuation carries a
            // bounded explanation instead of leaving it waiting without a source
            // of Monitor notifications. Do not spawn a second turn here.
            self.inner
                .wait_without_monitor
                .store(true, Ordering::Relaxed);
            return Ok(());
        }
        let Some(updated) = self
            .inner
            .state_dbs
            .thread_goals()
            .update_thread_goal(
                self.thread_id(),
                codex_state::GoalUpdate {
                    objective: None,
                    status: Some(codex_state::ThreadGoalStatus::GoalWait),
                    token_budget: None,
                    expected_goal_id: Some(goal.goal_id),
                },
            )
            .await
            .map_err(|err| err.to_string())?
        else {
            return Ok(());
        };
        self.inner.accounting_state.clear_active_goal();
        self.inner.analytics.status_changed(
            &updated,
            Some(goal.status),
            GoalEventAttribution::Turn(turn_id),
        );
        self.inner.event_emitter.thread_goal_updated(
            format!("{turn_id}:goal-wait"),
            Some(turn_id.to_string()),
            protocol_goal_from_state(updated),
        );
        Ok(())
    }
}
