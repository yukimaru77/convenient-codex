use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::parse_arguments;
use crate::tools::handlers::resolve_tool_environment;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use crate::unified_exec::ExecCommandRequest;
use crate::unified_exec::MonitorDelivery;
use crate::unified_exec::UnifiedExecContext;
use crate::unified_exec::spawn_delivery;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde::Deserialize;
use serde_json::json;

use super::ExecCommandArgs;
use super::ShellLocation;
use super::get_command;
use super::shell_mode_for_environment;

const MONITOR_TOOL_NAME: &str = "monitor";

/// Time the spawn blocks for initial output before returning. Kept short so the
/// tool call returns quickly while the watcher keeps running in the background.
const MONITOR_YIELD_MS: u64 = 250;

#[derive(Clone, Copy)]
pub enum MonitorHandler {
    Summary,
    Realtime,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum MonitorAction {
    Start,
    Stop,
    List,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MonitorArgs {
    action: MonitorAction,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    interval_minutes: Option<f64>,
}

fn create_monitor_tool(kind: MonitorHandler) -> ToolSpec {
    let mut properties = BTreeMap::from([
        (
            "action".to_string(),
            JsonSchema::string_enum(
                vec![json!("start"), json!("stop"), json!("list")],
                Some("Which monitor operation to perform.".to_string()),
            ),
        ),
        (
            "command".to_string(),
            JsonSchema::string(Some(
                "Shell command to run as the watcher (action=start). Captures stdout and stderr.".to_string(),
            )),
        ),
        (
            "description".to_string(),
            JsonSchema::string(Some(
                "Short label prefixed to every notification this watcher emits (action=start), e.g. \"errors in app.log\".".to_string(),
            )),
        ),
        (
            "id".to_string(),
            JsonSchema::string(Some(
                "The monitor id returned by a previous start (action=stop).".to_string(),
            )),
        ),
    ]);

    let (name, description) = match kind {
        MonitorHandler::Summary => {
            properties.insert("interval_minutes".to_string(), JsonSchema::number(Some("Positive notification interval in minutes (action=start), default 60. Fractional minutes are allowed.".to_string())));
            (
                MONITOR_TOOL_NAME,
                "Run a shell command as a background watcher. Every interval_minutes (default 60 minutes), send only the first 3 and last 20 lines of output accumulated since the previous notification, with an omission marker for skipped middle lines. Short batches are sent once without overlap; empty intervals are silent. Also send remaining output and exit status when the command exits. Long lines are truncated to 256 bytes. Prefer this tool for build, training, job, and log monitoring. action=start returns an id; action=stop stops a watcher; action=list lists watchers shared with monitor_realtime.",
            )
        }
        MonitorHandler::Realtime => (
            "monitor_realtime",
            "Discouraged for routine monitoring: frequent notifications consume many tokens and clutter the context. Use monitor instead unless immediate synchronization is essential, such as responding to a live incident or coordinating an interactive process. Runs a background shell command and forwards stdout/stderr lines, batching lines arriving within 200 ms. Also reports command exit. action=start returns an id; action=stop stops a watcher; action=list lists watchers shared with monitor. Output is bounded and a flood of 5000 lines automatically stops the watcher.",
        ),
    };

    ToolSpec::Function(ResponsesApiTool {
        name: name.to_string(),
        description: description.to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            properties,
            Some(vec!["action".to_string()]),
            /*additional_properties*/ Some(false.into()),
        ),
        output_schema: None,
    })
}

impl ToolExecutor<ToolInvocation> for MonitorHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(match self {
            Self::Summary => MONITOR_TOOL_NAME,
            Self::Realtime => "monitor_realtime",
        })
    }

    fn spec(&self) -> ToolSpec {
        create_monitor_tool(*self)
    }

    fn handle<'a>(&'a self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'a>
    where
        ToolInvocation: 'a,
    {
        Box::pin(handle_call(invocation, *self))
    }
}

impl CoreToolRuntime for MonitorHandler {
    fn pre_tool_use_payload(
        &self,
        invocation: &ToolInvocation,
    ) -> Option<crate::tools::registry::PreToolUsePayload> {
        let ToolPayload::Function { arguments } = &invocation.payload else {
            return None;
        };
        let args: MonitorArgs = parse_arguments(arguments).ok()?;
        if !matches!(args.action, MonitorAction::Start) {
            return None;
        }
        Some(crate::tools::registry::PreToolUsePayload {
            tool_name: crate::tools::hook_names::HookToolName::bash(),
            tool_input: json!({"command": args.command?}),
        })
    }

    fn with_updated_hook_input(
        &self,
        mut invocation: ToolInvocation,
        updated_input: serde_json::Value,
    ) -> Result<ToolInvocation, FunctionCallError> {
        let ToolPayload::Function { arguments } = &invocation.payload else {
            return Err(FunctionCallError::RespondToModel(
                "monitor requires function arguments".into(),
            ));
        };
        invocation.payload = ToolPayload::Function {
            arguments: crate::tools::handlers::rewrite_function_string_argument(
                arguments,
                "monitor",
                "command",
                crate::tools::handlers::updated_hook_command(&updated_input)?,
            )?,
        };
        Ok(invocation)
    }
}

async fn handle_call(
    invocation: ToolInvocation,
    kind: MonitorHandler,
) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
    let ToolInvocation {
        session,
        turn,
        step_context,
        cancellation_token,
        call_id,
        payload,
        ..
    } = invocation;
    let ToolPayload::Function { arguments } = payload else {
        return Err(FunctionCallError::RespondToModel(format!(
            "{MONITOR_TOOL_NAME} handler received unsupported payload"
        )));
    };
    let args: MonitorArgs = parse_arguments(&arguments)?;
    let delivery = match kind {
        MonitorHandler::Summary => {
            let minutes = args.interval_minutes.unwrap_or(60.0);
            let interval = Duration::try_from_secs_f64(minutes * 60.0)
                .ok()
                .filter(|duration| !duration.is_zero())
                .filter(|duration| tokio::time::Instant::now().checked_add(*duration).is_some())
                .ok_or_else(|| {
                    FunctionCallError::RespondToModel(
                        "interval_minutes must be positive and within the supported timer range"
                            .into(),
                    )
                })?;
            MonitorDelivery::Summary { interval }
        }
        MonitorHandler::Realtime => {
            if args.interval_minutes.is_some() {
                return Err(FunctionCallError::RespondToModel(
                    "interval_minutes is only supported by monitor".into(),
                ));
            }
            MonitorDelivery::Realtime
        }
    };

    match args.action {
        MonitorAction::Start => {
            start(
                &session,
                &turn,
                step_context,
                cancellation_token,
                call_id,
                args,
                delivery,
            )
            .await
        }
        MonitorAction::Stop => {
            let Some(id) = args.id else {
                return Err(FunctionCallError::RespondToModel(
                    "action=stop requires `id`".to_string(),
                ));
            };
            let message = match session.services.monitor_manager.remove(&id).await {
                Some(process_id) => {
                    session
                        .services
                        .unified_exec_manager
                        .terminate_process(process_id)
                        .await;
                    format!("Stopped monitor {id}.")
                }
                None => format!("No active monitor with id {id}."),
            };
            Ok(text_output(message))
        }
        MonitorAction::List => {
            let monitors = session.services.monitor_manager.list().await;
            let message = if monitors.is_empty() {
                "No active monitors.".to_string()
            } else {
                monitors
                    .iter()
                    .map(|m| {
                        format!(
                            "{}  [{}]  {}",
                            m.id,
                            m.description,
                            m.command.chars().take(80).collect::<String>()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            Ok(text_output(message))
        }
    }
}

async fn start(
    session: &Arc<crate::session::session::Session>,
    turn: &Arc<crate::session::turn_context::TurnContext>,
    step_context: Arc<crate::session::step_context::StepContext>,
    cancellation_token: tokio_util::sync::CancellationToken,
    call_id: String,
    args: MonitorArgs,
    delivery: MonitorDelivery,
) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
    let command = args
        .command
        .filter(|c| !c.trim().is_empty())
        .ok_or_else(|| {
            FunctionCallError::RespondToModel(
                "action=start requires a non-empty `command`".to_string(),
            )
        })?;
    let description = args
        .description
        .filter(|d| !d.trim().is_empty())
        .ok_or_else(|| {
            FunctionCallError::RespondToModel(
                "action=start requires a non-empty `description`".to_string(),
            )
        })?;
    if description.len() > 80 || command.len() > 8192 {
        return Err(FunctionCallError::RespondToModel(
            "Use a label of at most 80 bytes and a command of at most 8192 bytes.".into(),
        ));
    }
    if session.services.monitor_manager.list().await.len() >= 8 {
        return Err(FunctionCallError::RespondToModel(
            "At most 8 monitors may run per session. Stop one before starting another.".into(),
        ));
    }

    let manager = &session.services.unified_exec_manager;
    let context = UnifiedExecContext::new(
        session.clone(),
        step_context.clone(),
        cancellation_token,
        call_id,
    );
    let Some(turn_environment) = resolve_tool_environment(
        session,
        turn,
        &step_context.environments,
        /*environment_id*/ None,
    )
    .await?
    else {
        return Err(FunctionCallError::RespondToModel(
            "unified exec is unavailable in this session".to_string(),
        ));
    };
    let cwd = turn_environment.cwd().clone();
    let environment = Arc::clone(&turn_environment.environment);
    let shell_mode =
        shell_mode_for_environment(&turn.unified_exec_shell_mode, environment.as_ref());
    let shell = turn_environment
        .shell
        .clone()
        .map(Arc::new)
        .unwrap_or_else(|| session.user_shell());

    // Resolve `command` to a concrete shell invocation with the session default
    // shell and no permission escalation; the monitor only needs the resolved
    // command + shell type back from `get_command`.
    let exec_args = ExecCommandArgs {
        cmd: command.clone(),
        shell: None,
        login: None,
        tty: false,
        yield_time_ms: 0,
        timeout_ms: None,
        max_output_tokens: None,
        sandbox_permissions: Default::default(),
        additional_permissions: None,
        justification: None,
        prefix_rule: None,
    };
    let resolved = get_command(
        &exec_args,
        shell,
        &shell_mode,
        turn_environment.config().allow_login_shell,
        if environment.is_remote() {
            ShellLocation::Remote
        } else {
            ShellLocation::Local
        },
    )
    .map_err(FunctionCallError::RespondToModel)?;

    let process_id = manager.allocate_process_id().await;
    let request = ExecCommandRequest {
        command: resolved.command,
        shell_type: resolved.shell_type,
        hook_command: command.clone(),
        process_id,
        yield_time_ms: MONITOR_YIELD_MS,
        max_output_tokens: None,
        cwd: cwd.clone(),
        sandbox_cwd: cwd,
        turn_environment: turn_environment.clone(),
        shell_mode,
        network: turn.network.clone(),
        tty: false,
        sandbox_permissions: Default::default(),
        additional_permissions: None,
        additional_permissions_preapproved: false,
        justification: None,
        prefix_rule: None,
    };

    let (initial_output, process) = match manager.exec_monitor_command(request, &context).await {
        Ok(output) => output,
        Err(err) => {
            manager.release_process_id(process_id).await;
            return Err(FunctionCallError::RespondToModel(format!(
                "failed to start monitor: {err:?}"
            )));
        }
    };

    // Hand off the initial yield's captured bytes and continue draining the
    // same process buffer. Retain short-lived processes and register the task
    // before allowing delivery, including its final exit notification.
    let id = format!("mon_{}", uuid::Uuid::new_v4());
    let (task, ready_tx) = spawn_delivery(
        process,
        process_id,
        id.clone(),
        Arc::downgrade(session),
        description.clone(),
        initial_output.raw_output,
        delivery,
    );

    session
        .services
        .monitor_manager
        .insert(
            id.clone(),
            process_id,
            description.clone(),
            command,
            delivery,
            task,
        )
        .await;
    let _ = ready_tx.send(());
    Ok(text_output(format!(
        "Started monitor {id}: watching \"{description}\". Stop it with action=stop, id={id}."
    )))
}

fn text_output(message: String) -> Box<dyn crate::tools::context::ToolOutput> {
    boxed_tool_output(FunctionToolOutput::from_text(
        message,
        /*success*/ Some(true),
    ))
}
