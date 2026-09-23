# GOAL_WAIT: suspend automatic continuation, wake on an external turn

Requested 2026-09-12. This change is independent of Monitor presence: Monitor keeps reporting results; it does not control Goal continuation directly.

- The persisted Goal state gains `goal_wait` (`goalWait` in the camel-case protocol).
- Only active Goals automatically continue. One completed turn whose final returned assistant text contains the case-sensitive marker `GOAL_WAIT` enters this state. Surrounding text, Markdown and newlines are accepted. The user's latest revisions changed exact matching to contains, and three consecutive automatic turns to one turn (including user/Monitor turns). There is no counter.
- Starting a user, Monitor, or other non-Goal turn wakes only `goal_wait` to `active`. It does not manufacture an extra turn. Paused, blocked, usage-limited, budget-limited and completed Goals retain their state.
- Reopening a session without starting a turn does not wake a waiting Goal. The state persists in SQLite.
- Monitor tools remain start/stop/list; no wait/continue action is added. No process is paused, killed or restarted by this feature.
- The continuation prompt retains the condition: no useful independent work or useful further investigation currently remains, and necessary external results are configured to arrive through Monitor. The runtime recognizes the signal; it does not attempt to judge research value or inspect Monitor handles.

Implementation is in the Goal extension, with narrow turn-lifecycle inputs from core, a new status across protocol/state/UI, and a migration preserving existing Goals and fork continuation deferrals.

## Validation and installation

The final one-response implementation passed 404 scoped tests: Goal extension 36, app-server Goal/Monitor 4, TUI Goal menu 13, migration 1, extension API 16, protocol 334. One existing realtime-Monitor app-server test failed its initialization attempt and passed nextest's retry; this is not an all-first-attempt pass. The two new app-server tests exercise actual user turns and a real Monitor process exit. The migration test checks preservation of existing Goals and continuation deferrals. Schema fixtures were regenerated through the repository's ignored fixture-generation test because the old justfile recipe names a removed binary.

`build-one-shot.sh` / `build-one-shot.log` are the definitive latest-semantics run, ending `ONE_SHOT_BUILD_COMPLETE` and `BUILD_EXIT=0`. Earlier logs retain the superseded three-response test and development failures; do not use them as evidence of the latest behavior. No full-workspace test was run. An unrelated unused-import edit made by clippy was restored.

`install.sh` was executed successfully. The new executable was ad-hoc signed, verified and installed at `/opt/homebrew/lib/node_modules/@openai/codex/node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex`. Its previous executable is retained at `/Users/nonaka/.local/share/codex-monitor/v0.153.4/goal-wait-20260912/codex.before-goal-wait`. The version string remains `codex-cli 0.153.4`.

## Actual herdr test, 2026-09-12 JST

New isolated tab `w2B:tF`, pane `w2B:pK`, Codex session `01a0956a-1983-73f2-90c0-93e2d796303c`, model `gpt-6-astra` / low. The installed executable ran as PID 63751. CLI config overrides select the new embedded app-server instead of reusing an older shared daemon, and isolate SQLite under this test's `live/sqlite`. The test's real native `/goal` acceptance was confirmed in the UI and Goal database, not inferred from prompt delivery.

1. At 20:40:05 the first completed response was `Runtime test is waiting: GOAL_WAIT`. SQLite showed `goal_wait`; the actual herdr footer showed `Goal waiting (resumes on a new turn)` with the finite buffered Monitor still running. Surrounding prose did not prevent detection.
2. A user turn at 20:40:41 called `get_goal`, which returned `active` without an explicit resume. Its final response contained additional prose and the marker; SQLite again showed `goal_wait`.
3. The harness released the Monitor fixture's FIFO, not the model. The real process exited 0 with `GOAL_WAIT_LIVE_PROCESS_SUCCESS`. Its notification started the third turn at 20:41:14; `get_goal` again returned `active`. The model then marked the test Goal complete; SQLite and the UI confirmed completion.

The rollout contains exactly three started/completed turns: initial Goal, external user input, and actual Monitor exit. There were no extra automatic continuations in the two observed waiting intervals. The Monitor default is 60 minutes; this smoke test verifies terminal delivery, not a one-hour aggregate. No monitor remained active after completion.

Live evidence: `live/stage2-user-wake.txt`, `live/stage3-monitor-wake.txt`, `live/events.jsonl`, `live/final-goal.json`, and the fixture/launch scripts. Original full rollout: `/Users/nonaka/.codex/sessions/2026/09/12/rollout-2026-09-12T20-39-13-01a0956a-1983-73f2-90c0-93e2d796303c.jsonl`. The initial prompt was submitted before CLI initialization completed and herdr reported `agent_prompt_stalled`; after readiness the existing composer was submitted, not duplicated. The model's first Monitor call omitted its required description and was rejected; its corrected call created exactly one actual Monitor. Both setup errors remain in the evidence.

The marker remains visible in the assistant's response; hiding it was not implemented. Detection is on the completed turn's returned assistant text, not arbitrary user text or tool outputs. Reopening a waiting session without starting a turn leaves it waiting. Existing research sessions, Goals, compute jobs, and the old shared daemon were not restarted. They must actually load the new runtime to receive this behavior.

Source snapshot in the shared record preserves the touched files as built, including pre-existing user modifications in those files. It is not a clean patch against upstream: the working checkout already had custom Monitor/TUI changes. Generated schema files can be regenerated using the command in `build.sh`. Research remains paused.
