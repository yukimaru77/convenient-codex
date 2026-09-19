# Local Monitor implementation

This build also restores the user's env switch feature; see `ENV_SWITCH.md`
for routing behavior and the combined Docker/Monitor integration test.

Based on the official `rust-v0.153.4` release (3d2ee51ca2).
The watcher design was adapted from yaanfpv/codex commit
ae7dbe6aecf15dd7c1747a8512acda004d38d6a5, linked in openai/codex#29922.

Enable the tool in `~/.codex/config.toml`:

```toml
[features]
monitor = true
```

The model can call `monitor` (default summary interval: 60 minutes):

```json
{"action":"start","command":"tail -F app.log | grep --line-buffered ERROR","description":"application errors"}
{"action":"list"}
{"action":"stop","id":"mon_<id returned by start>"}
```

Set `interval_minutes` on start to choose a positive interval, including
fractional minutes. Each nonempty interval sends its first 3 and last 20 output
lines, with `... (N lines omitted) ...` when middle lines are skipped. Batches
of 23 lines or fewer are sent without overlap or an omission marker. The batch
resets after delivery. Pending unterminated text is included at the interval
boundary, and remaining output is also delivered on normal command exit.

`monitor_realtime` retains the previous 200 ms line batching. It is discouraged
for routine monitoring because frequent notifications consume tokens and clutter
context. Reserve it for immediate synchronization, such as live incident response
or interactive process coordination. Both tools share start/list/stop and the
same monitor registry; only `monitor` accepts `interval_minutes`.

Monitoring and waiting are separate. Starting `monitor` or `monitor_realtime`
does not suppress an active Goal's continuation. The model can keep working or
launch another useful independent task while watchers run.

The Goal continuation predicate does not consult the monitor registry. Native
Goal status, queued work, and the ordinary turn scheduling rules still apply.
The previously added `wait`/`continue` actions and wait state were removed;
the monitor API is again start/list/stop only. Output buffering and terminal
notifications are unchanged. No repeated-response waiting mechanism is added
in this incremental change. Until such a mechanism exists, monitoring alone
does not provide automatic waiting. User-paused Goals remain paused.

The embedded continuation template describes these rules. Running old binaries
retain their existing behavior until a new binary/session is used.

The TUI status row displays live monitor counts beside the goal/mode indicator:
`monitor ×2 (60m) · monitor_realtime ×1`. Summary monitors are cyan and realtime
monitors are magenta. Different summary intervals display their minimum–maximum
range. The selected thread's live process snapshot is refreshed about once per
second through `thread/backgroundTerminals/list`, without invoking the model or
adding context. Exited/stopped monitors disappear; changing threads or going
offline clears the previous snapshot. Older servers without monitor metadata
continue to work without this indicator.

The command uses the selected environment's shell, working directory, and
unified-exec sandbox/approval path. Start calls also run Bash PreToolUse hooks.
Stdout and stderr share the monitored output stream. The initial yield hands
off the same buffered output to avoid missing early bytes. Immediately exiting
commands are supported.

Notifications enter the session's pending-work queue. Active turns receive them
at safe model-call boundaries; idle sessions resume through the existing
scheduler. Quiet monitors generate no model calls. Normal exit includes the exit
status; explicit stop aborts delivery and terminates the process without waking
the agent. Session shutdown cancels all monitors.

Resource bounds: eight monitors per session, 80-byte labels, 8192-byte commands,
32 queued notifications. Summary mode retains only 23 lines plus a partial line,
with each line capped at 256 UTF-8 bytes (plus a truncation marker), and an
8192-byte notification body cap. Realtime mode retains its 700-byte body cap and
5000-line automatic flood stop. Summary mode does not stop after 5000 lines.
Truncation and queue overflow are visible.

This is a native core tool, not an MCP server or a watcher subagent. It applies
to new sessions using the patched CLI/app-server binary. Existing processes
retain their original executable and tool definitions. An npm update may replace
the patched executable. The separately bundled ChatGPT desktop binary is not
modified by this CLI installation.

Validation commands:

```sh
cd codex-rs
just test -p codex-core -E 'test(monitor) | test(input_queue) | test(unified_exec)' --retries 0
cargo build -p codex-cli --bin codex
just write-config-schema
just fix -p codex-core
just fmt
```

Before the summary/realtime split, the focused regression run passed 185 tests, including idle/no-inference,
exactly-once completion, immediate exit, partial lines, stdout/stderr, stopping,
registry cleanup, bounded context, overflow, and read-only sandbox enforcement.

The summary/realtime split passed 197 focused tests with retries disabled,
covering Monitor, the pending-input queue, and unified exec. New coverage includes
default interval silence followed by exit head/tail delivery, fractional-minute
periodic delivery (including text without a newline), empty interval silence,
batch reset, nonpositive interval rejection before process spawn, and bounded
head/tail retention across more than 5000 lines and invalid UTF-8 output.

Before this split, the full workspace run completed: 16,917 passed, 58 failed, 4 timed out,
44 skipped (16,979 executed). All 12 Monitor integration tests passed in that
run too. A low-concurrency follow-up of selected failing families ran 33 tests:
19 passed and 14 failed. The complete workspace suite is **not green**.

Observed failures outside the added Monitor tests include release-version
expectations (`0.0.0` versus `0.153.4`) in snapshots/MCP initialization, personal
Mac skills appearing in skill discovery fixtures, Apple Python cache warnings
inside Seatbelt, startup/HTTP timing, and a V8 sandbox-feature assertion for
the official prebuilt V8 archive. Not every remaining failure has been isolated
against an unmodified upstream build; these are not claimed to be proven
pre-existing failures. No unrelated snapshots or feature behavior were changed
to make the suite pass.

Full-workspace tests used the checksummed OpenAI V8 artifacts resolved by
`scripts/codex_package/v8.py`, since the default denoland archive URL returned
404. The CLI build itself does not require that V8 override.

## Installed on this Mac

The research-focused Goal continuation prompt is installed, SHA-256
`9f8b30537ed47273bbf30ccf69d93e00812bef2dd7f84a932a0c40cb595205b8`.
The previous binary/template and validation logs are in
`~/.local/share/codex-monitor/v0.153.4/research-continuation/`.
The replacement translates the user's complete research instructions; the
requested `monitor_runtime` name is normalized to the actual `monitor_realtime`
tool. All 34 Goal tests passed, as did formatting, CLI build/startup, signature
verification, and embedded-prompt checks. The bilingual Tailscale page was
updated and fetched over HTTPS to verify both full texts. New CLI processes use
the replacement; already-running sessions retain their embedded template.

The Goal monitor gate is installed, SHA-256
`2cd404a3c3a7eec0d4c7afe4aaa6fdcb2b4aa286eb25e504eaff651b51adc242`.
Backup and logs: `~/.local/share/codex-monitor/v0.153.4/goal-gate-20260909-070648/`.
All 34 Goal tests and three live app-server integration cases passed. The latter
verify waiting with an active Goal and resuming after summary/realtime exit or
an explicit monitor stop turn. Formatting, CLI build/startup, and signature
verification passed. Start a new CLI session to use this condition.

The live Monitor TUI footer is installed in the npm CLI executable. Its SHA-256
is `64ac77a4f7054f9b917f31c9bb09b0b3d1d03a99909f78026166069b52587360`.
The previous binary, changed source files, and validation logs are retained in
`/Users/nonaka/.local/share/codex-monitor/v0.153.4/footer-20260909-063620/`.
Validation passed: 38 focused Monitor/schema tests, 140 footer/status tests,
and 299 protocol tests (one ignored). `just fix` completed; its disallowed-yellow
warning was resolved by using magenta. Formatting, CLI build, signature, and
CLI startup were verified. Start a new CLI session to use the footer.

The summary/realtime split is installed in the npm CLI executable. The previous
binary, source patch, and build/lint logs are retained in
`/Users/nonaka/.local/share/codex-monitor/v0.153.4/summary-20260909-031614/`.
That earlier build's SHA-256 is
`35efccf645b8b6b8a14b95a3a953fcd7a6bdd465d8a926e9b92eacb41a008e54`.
The final `cargo clippy -p codex-core --lib` completed without warnings; formatting,
CLI build, ad-hoc signature verification, CLI startup, and the enabled Monitor
feature were also verified. A new CLI session is required to use the new tool
definitions. The full workspace suite was not rerun for this split.

The npm CLI native executable was replaced atomically, leaving running sessions
intact. The signed executable reports `codex-cli 0.153.4`; `codex features list`
reports `monitor` as enabled. `~/.codex/config.toml` has `features.monitor = true`.

Source: `/Users/nonaka/tasks/codex-monitor-v0.153.4`, branch `monitor-v0.153.4`.
Backups: `/Users/nonaka/.local/share/codex-monitor/v0.153.4/codex.stock` and
`config.toml.before-monitor` in the same directory. The installed build is also
retained there as `codex.monitor` (the initial Monitor-only build). The combined
env-switch/Monitor build is saved as `codex.env-switch-monitor`; both are
stripped, ad-hoc-signed development builds.
Validation logs are in the `validation/` subdirectory and generated, unaccepted
snapshot results are in `test-snapshots/`. The initial Monitor-only build's
Cargo artifacts were removed to recover disk space. The subsequent combined
build uses smaller build settings and retains its artifacts; see `ENV_SWITCH.md`.

To revert the CLI, copy `codex.stock` to a new sibling file in
`/opt/homebrew/lib/node_modules/@openai/codex/node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/`,
then atomically rename that copy to `codex`. Remove only `monitor = true` from
the config, preserving subsequent configuration changes. Do not overwrite the
whole configuration from the backup unless you intend to discard later edits.
