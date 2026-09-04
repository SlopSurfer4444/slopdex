use super::*;
use crate::session::InputQueueActivity;
use crate::tools::handlers::multi_agents_spec::WaitAgentTimeoutOptions;
use crate::tools::handlers::multi_agents_spec::create_wait_agent_tool_v2;
use codex_tools::ToolSpec;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::Instant;
use tokio::time::timeout_at;

#[derive(Default)]
pub(crate) struct Handler {
    options: WaitAgentTimeoutOptions,
}

impl Handler {
    pub(crate) fn new(options: WaitAgentTimeoutOptions) -> Self {
        Self { options }
    }
}

impl ToolExecutor<ToolInvocation> for Handler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("wait_agent")
    }

    fn spec(&self) -> ToolSpec {
        create_wait_agent_tool_v2(self.options)
    }

    fn handle<'a>(&'a self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'a>
    where
        ToolInvocation: 'a,
    {
        Box::pin(self.handle_call(invocation))
    }
}

impl Handler {
    async fn handle_call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
        let ToolInvocation {
            session,
            turn,
            payload,
            call_id,
            ..
        } = invocation;
        let arguments = function_arguments(payload)?;
        let args: WaitArgs = parse_arguments(&arguments)?;
        let target_ids = if let Some(targets) = args.targets {
            if targets.is_empty() {
                return Err(FunctionCallError::RespondToModel(
                    "targets must contain at least one direct child".to_string(),
                ));
            }
            let parent_path = turn
                .session_source
                .get_agent_path()
                .unwrap_or_else(AgentPath::root);
            let control = &session.services.agent_control;
            let mut target_ids = Vec::with_capacity(targets.len());
            let mut seen = std::collections::HashSet::with_capacity(targets.len());
            for target in targets {
                let thread_id = resolve_agent_target(&session, &turn, &target).await?;
                if !seen.insert(thread_id) {
                    return Err(FunctionCallError::RespondToModel(format!(
                        "target `{target}` resolves to a duplicate child"
                    )));
                }
                let metadata = control
                    .ensure_agent_known(thread_id)
                    .map_err(|err| collab_agent_error(thread_id, err))?;
                let child_path = metadata.agent_path.ok_or_else(|| {
                    FunctionCallError::RespondToModel(format!(
                        "target `{target}` is missing an agent path"
                    ))
                })?;
                if !super::join::is_direct_child(&parent_path, &child_path) {
                    return Err(FunctionCallError::RespondToModel(format!(
                        "target `{target}` is not a direct child of {parent_path}"
                    )));
                }
                target_ids.push(thread_id);
            }
            Some(target_ids)
        } else {
            None
        };
        let min_timeout_ms = turn.config.multi_agent_v2.min_wait_timeout_ms;
        let max_timeout_ms = turn.config.multi_agent_v2.max_wait_timeout_ms;
        let default_timeout_ms = turn.config.multi_agent_v2.default_wait_timeout_ms;
        let requested_timeout_ms = args.timeout_ms;
        let timeout_ms = match requested_timeout_ms {
            Some(ms) if ms > max_timeout_ms => {
                return Err(FunctionCallError::RespondToModel(format!(
                    "timeout_ms must be at most {max_timeout_ms}"
                )));
            }
            Some(ms) => ms.max(min_timeout_ms),
            None => default_timeout_ms,
        };

        let mut ready_result = None;
        if let Some(target_ids) = target_ids.as_ref() {
            let ready_outcomes = session
                .services
                .agent_control
                .consume_ready_join_obligation_for_targets_with_outcomes(
                    session.thread_id,
                    &turn.sub_id,
                    target_ids,
                )
                .await
                .unwrap_or(None);
            if let Some(outcomes) = ready_outcomes {
                let mut result = WaitAgentResult::from_outcome(
                    WaitOutcome::MailboxActivity,
                    requested_timeout_ms,
                    timeout_ms,
                );
                let outcomes = outcomes
                    .iter()
                    .map(|(thread_id, status)| format!("{thread_id}: {status:?}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                result
                    .message
                    .push_str(&format!("\n\nNative child outcomes: {outcomes}"));
                ready_result = Some(result);
            } else {
                let registered = session
                    .services
                    .agent_control
                    .register_wait_obligation(
                        session.thread_id,
                        turn.sub_id.clone(),
                        target_ids.clone(),
                    )
                    .await
                    .map_err(|err| FunctionCallError::RespondToModel(err.to_string()))?;
                if !registered {
                    return Err(FunctionCallError::RespondToModel(
                        "targetful wait could not bind the requested current child turns"
                            .to_string(),
                    ));
                }
            }
        }

        session
            .emit_turn_item_started(
                &turn,
                &TurnItem::CollabAgentToolCall(CollabAgentToolCallItem {
                    id: call_id.clone(),
                    tool: CollabAgentTool::Wait,
                    status: CollabAgentToolCallStatus::InProgress,
                    sender_thread_id: session.thread_id,
                    receiver_thread_ids: Vec::new(),
                    receiver_agents: Vec::new(),
                    prompt: None,
                    model: None,
                    reasoning_effort: None,
                    agents_states: Default::default(),
                }),
            )
            .await;

        if let Some(result) = ready_result {
            session
                .emit_turn_item_completed(
                    &turn,
                    TurnItem::CollabAgentToolCall(CollabAgentToolCallItem {
                        id: call_id,
                        tool: CollabAgentTool::Wait,
                        status: CollabAgentToolCallStatus::Completed,
                        sender_thread_id: session.thread_id,
                        receiver_thread_ids: Vec::new(),
                        receiver_agents: Vec::new(),
                        prompt: None,
                        model: None,
                        reasoning_effort: None,
                        agents_states: HashMap::new(),
                    }),
                )
                .await;
            return Ok(boxed_tool_output(result));
        }

        let turn_state = session
            .input_queue
            .turn_state_for_sub_id(&session.active_turn, &turn.sub_id)
            .await;
        let (mut activity_rx, pending_activity) = session
            .input_queue
            .subscribe_activity(turn_state.as_deref())
            .await;

        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        let mut native_outcomes = None;
        let outcome = if let Some(target_ids) = target_ids {
            let mut pending_activity = pending_activity;
            let initial_outcomes = session
                .services
                .agent_control
                .consume_ready_join_obligation_for_targets_with_outcomes(
                    session.thread_id,
                    &turn.sub_id,
                    &target_ids,
                )
                .await
                .unwrap_or(None);
            if let Some(outcomes) = initial_outcomes {
                native_outcomes = Some(outcomes);
                WaitOutcome::MailboxActivity
            } else {
                loop {
                    let outcome =
                        wait_for_activity(&mut activity_rx, pending_activity, deadline).await;
                    pending_activity = None;
                    match outcome {
                        WaitOutcome::MailboxActivity => {
                            // Ignore unrelated mailbox activity. A targetful wait
                            // succeeds only when the exact retained obligation is
                            // consumed; timeout remains non-terminal.
                            let outcomes = session
                                .services
                                .agent_control
                                .consume_ready_join_obligation_for_targets_with_outcomes(
                                    session.thread_id,
                                    &turn.sub_id,
                                    &target_ids,
                                )
                                .await
                                .unwrap_or(None);
                            if let Some(outcomes) = outcomes {
                                native_outcomes = Some(outcomes);
                                break WaitOutcome::MailboxActivity;
                            }
                        }
                        WaitOutcome::Steered | WaitOutcome::TimedOut => break outcome,
                    }
                }
            }
        } else {
            wait_for_activity(&mut activity_rx, pending_activity, deadline).await
        };
        let mut result = WaitAgentResult::from_outcome(outcome, requested_timeout_ms, timeout_ms);
        if let Some(outcomes) = native_outcomes {
            let outcomes = outcomes
                .iter()
                .map(|(thread_id, status)| format!("{thread_id}: {status:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            result
                .message
                .push_str(&format!("\n\nNative child outcomes: {outcomes}"));
        }

        session
            .emit_turn_item_completed(
                &turn,
                TurnItem::CollabAgentToolCall(CollabAgentToolCallItem {
                    id: call_id,
                    tool: CollabAgentTool::Wait,
                    status: CollabAgentToolCallStatus::Completed,
                    sender_thread_id: session.thread_id,
                    receiver_thread_ids: Vec::new(),
                    receiver_agents: Vec::new(),
                    prompt: None,
                    model: None,
                    reasoning_effort: None,
                    agents_states: HashMap::new(),
                }),
            )
            .await;

        Ok(boxed_tool_output(result))
    }
}

impl CoreToolRuntime for Handler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WaitArgs {
    targets: Option<Vec<String>>,
    timeout_ms: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct WaitAgentResult {
    pub(crate) message: String,
    pub(crate) timed_out: bool,
}

impl WaitAgentResult {
    fn from_outcome(
        outcome: WaitOutcome,
        requested_timeout_ms: Option<i64>,
        timeout_ms: i64,
    ) -> Self {
        let message = match outcome {
            WaitOutcome::MailboxActivity => "Wait completed.",
            WaitOutcome::Steered => "Wait interrupted by new input.",
            WaitOutcome::TimedOut => "Wait timed out.",
        };
        let message = match requested_timeout_ms {
            Some(requested_timeout_ms) if requested_timeout_ms < timeout_ms => format!(
                "{message}\n\nRequested timeout of {requested_timeout_ms}ms was clamped to the minimum of {timeout_ms}ms."
            ),
            Some(_) | None => message.to_string(),
        };
        Self {
            message,
            timed_out: outcome == WaitOutcome::TimedOut,
        }
    }
}

impl ToolOutput for WaitAgentResult {
    fn log_output(&self) -> String {
        tool_output_json_text(self, "wait_agent")
    }

    fn success_for_logging(&self) -> bool {
        true
    }

    fn to_response_item(&self, call_id: &str, payload: &ToolPayload) -> ResponseInputItem {
        tool_output_response_item(call_id, payload, self, /*success*/ None, "wait_agent")
    }

    fn code_mode_result(&self, _payload: &ToolPayload) -> JsonValue {
        tool_output_code_mode_result(self, "wait_agent")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WaitOutcome {
    MailboxActivity,
    Steered,
    TimedOut,
}

async fn wait_for_activity(
    activity_rx: &mut tokio::sync::watch::Receiver<InputQueueActivity>,
    pending_activity: Option<InputQueueActivity>,
    deadline: Instant,
) -> WaitOutcome {
    if let Some(activity) = pending_activity {
        return match activity {
            InputQueueActivity::Mailbox => WaitOutcome::MailboxActivity,
            InputQueueActivity::Steer => WaitOutcome::Steered,
        };
    }
    match timeout_at(deadline, activity_rx.changed()).await {
        Ok(Ok(())) => match *activity_rx.borrow_and_update() {
            InputQueueActivity::Mailbox => WaitOutcome::MailboxActivity,
            InputQueueActivity::Steer => WaitOutcome::Steered,
        },
        Ok(Err(_)) | Err(_) => WaitOutcome::TimedOut,
    }
}
