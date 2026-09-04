use super::*;
use crate::tools::handlers::multi_agents_spec::create_join_agents_tool_v2;
use codex_tools::ToolSpec;

pub(crate) struct Handler;

impl ToolExecutor<ToolInvocation> for Handler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("join_agents")
    }

    fn spec(&self) -> ToolSpec {
        create_join_agents_tool_v2()
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
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        let ToolInvocation {
            session,
            turn,
            payload,
            ..
        } = invocation;
        let arguments = function_arguments(payload)?;
        let args: JoinAgentsArgs = parse_arguments(&arguments)?;
        if args.targets.is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "targets must contain at least one direct child".to_string(),
            ));
        }
        if args.condition != JoinCondition::All {
            return Err(FunctionCallError::RespondToModel(
                "only the `all` join condition is currently supported".to_string(),
            ));
        }

        let parent_path = turn
            .session_source
            .get_agent_path()
            .unwrap_or_else(AgentPath::root);
        let control = &session.services.agent_control;
        let mut child_thread_ids = Vec::with_capacity(args.targets.len());
        let mut seen = std::collections::HashSet::with_capacity(args.targets.len());
        for target in args.targets {
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
            if !is_direct_child(&parent_path, &child_path) {
                return Err(FunctionCallError::RespondToModel(format!(
                    "target `{target}` is not a direct child of {parent_path}"
                )));
            }
            child_thread_ids.push(thread_id);
        }

        let registered = control
            .register_join_obligation(session.thread_id, turn.sub_id.clone(), child_thread_ids)
            .await
            .map_err(|err| FunctionCallError::RespondToModel(err.to_string()))?;
        if !registered {
            return Err(FunctionCallError::RespondToModel(
                "one or more direct children no longer have the bound running turn, or an explicit child join is already active for this parent turn".to_string(),
            ));
        }
        Ok(boxed_tool_output(JoinAgentsResult {
            message: "Join registration accepted; the parent continuation remains available while direct children finish."
                .to_string(),
            suspended: false,
        }))
    }
}

impl CoreToolRuntime for Handler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }
}

pub(super) fn is_direct_child(parent: &AgentPath, child: &AgentPath) -> bool {
    child
        .as_str()
        .strip_prefix(&format!("{parent}/"))
        .is_some_and(|suffix| !suffix.is_empty() && !suffix.contains('/'))
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct JoinAgentsArgs {
    targets: Vec<String>,
    condition: JoinCondition,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum JoinCondition {
    All,
}

#[derive(Debug, Serialize)]
struct JoinAgentsResult {
    message: String,
    suspended: bool,
}

impl ToolOutput for JoinAgentsResult {
    fn log_output(&self) -> String {
        tool_output_json_text(self, "join_agents")
    }

    fn success_for_logging(&self) -> bool {
        true
    }

    fn to_response_item(&self, call_id: &str, payload: &ToolPayload) -> ResponseInputItem {
        tool_output_response_item(call_id, payload, self, Some(true), "join_agents")
    }

    fn code_mode_result(&self, _payload: &ToolPayload) -> JsonValue {
        tool_output_code_mode_result(self, "join_agents")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_targets_only_direct_children() {
        let parent = AgentPath::try_from("/root/parent").expect("parent path");
        let child = AgentPath::try_from("/root/parent/child").expect("child path");
        let grandchild =
            AgentPath::try_from("/root/parent/child/grandchild").expect("grandchild path");

        assert!(is_direct_child(&parent, &child));
        assert!(!is_direct_child(&parent, &grandchild));
    }
}
