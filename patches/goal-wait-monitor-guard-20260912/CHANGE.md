# GOAL_WAIT requires this session's Monitor

2026-09-12 user revision: when a final response contains GOAL_WAIT but the session has zero active Monitors, continue with an explanation rather than becoming indefinitely waiting.

- Core passes the current session's registered Monitor delivery-task count to turn-stop contributors. Other sessions' monitors do not count. Both buffered and legacy realtime registrations count; buffered remains preferred. This is a registry snapshot, not a guarantee of useful computation.
- The Goal extension leaves an active Goal active when the count is zero. It does not briefly enter goal_wait or create a second execution path. The ordinary native continuation includes a bounded missing-Monitor explanation. Already delivered results should be used; only actual required external work should get a real finite Monitor. Dummy watchers and duplicate jobs are explicitly discouraged.
- A session with a Monitor retains the previous one-response substring behavior. Native paused/blocked/limited/complete states are untouched. Monitor presence alone still does not suppress continuation: the assistant must explicitly request waiting.
- The correction is consumed after a continuation is actually submitted, not on a failed submission. An intervening external turn or Goal mutation clears it. No schema migration, extra Goal state, new Monitor action, or polling was added.

`build.sh` completed with exit 0: Goal extension 37, extension API 16, app-server Goal/Monitor 5, and core Monitor 8 tests passed (66 total). One existing realtime-Monitor initialization test needed nextest's retry; the new no-Monitor recovery and both real-Monitor wait/wake tests passed on their first attempts. `just fix` and CLI build succeeded; an unrelated import removal by clippy was restored. No full-workspace test was run.

`install.sh` signed, verified and installed the new CLI successfully. The prior executable is retained at `/Users/nonaka/.local/share/codex-monitor/v0.153.4/goal-wait-monitor-guard-20260912/codex.before-monitor-guard`. The version label remains 0.153.4. `monitor-guard.patch` is the incremental change against the immediately preceding GOAL_WAIT implementation; `save-patch.sh` generated it from the captured pre-change files. Reverse-apply checking against the current source passed.

## Real herdr test: passed

New tab `w2B:tG`, pane `w2B:pM`, PID 75569, session `01a09593-feb8-7c53-bd52-8bcefb87af44`, `gpt-6-astra` / low. CLI overrides select the newly installed embedded backend; no old shared daemon was reused. The previous isolated test SQLite directory was reused, not any research Goal database.

At 21:26:04.893 JST the first turn ended with `Negative-path input: GOAL_WAIT`, after a real `monitor list` returned no active monitors. At 21:26:04.920 a native automatic continuation started (27 ms later), without harness input. The actual request carried `GOAL_WAIT was not accepted: there are no active Monitors in this session.` The model quoted that correction, and `get_goal` returned active.

That second turn registered exactly one finite FIFO-backed Monitor and finished with `Real process pending: GOAL_WAIT`. The Goal database showed `goal_wait`, and herdr displayed `monitor ×1 (60m) | Goal waiting (resumes on a new turn)`. The harness then released the real process; its exit-0 notification started the third turn. `get_goal` returned active, and the test was marked complete. Final SQLite state is complete. Exactly three started/completed turns were recorded; no extra continuation occurred during the legitimate waiting interval.

Evidence is under `live/`: runtime notice, actual TUI text before/after Monitor exit, filtered tool/turn events and final Goal state. Original rollout: `/Users/nonaka/.codex/sessions/2026/09/12/rollout-2026-09-12T21-24-59-01a09593-feb8-7c53-bd52-8bcefb87af44.jsonl`. The native continuation carries the explanation as internal model context; this is not a separate fixed TUI warning banner. The live model visibly quoted it.

No Monitors remained active after completion. The test Codex alone was exited with `/quit`. Existing research sessions, Goals, compute jobs, and the shared daemon remain untouched. Code/logs/evidence are shared on the private `research/input-target-use-20260912` branch, separate from main.
