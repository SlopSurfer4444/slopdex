//! Turn-scoped state and active turn metadata scaffolding.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;

use codex_diagnostics::GaugeGuard;
use codex_protocol::dynamic_tools::DynamicToolResponse;
use codex_protocol::protocol::TurnEnvironmentSelection;
use codex_protocol::request_permissions::RequestPermissionProfile;
use codex_protocol::request_permissions::RequestPermissionsResponse;
use codex_protocol::request_user_input::RequestUserInputResponse;
use codex_rmcp_client::ElicitationResponse;
use codex_sandboxing::policy_transforms::merge_permission_profiles;
use rmcp::model::RequestId;
use tokio::sync::oneshot;

use crate::agent::control::AgentExecutionGuard;
use crate::mcp_tool_call::McpToolApprovalMetadata;
use crate::session::TurnInputQueue;
use crate::session::step_context::StepContext;
use crate::session::turn_context::TurnContext;
use crate::tasks::AgentExecutionReservation;
use crate::tasks::AnySessionTask;
use crate::tasks::TasklessTurnClaim;
use codex_protocol::models::AdditionalPermissionProfile;
use codex_protocol::protocol::McpInvocation;
use codex_protocol::protocol::ReviewDecision;
use codex_protocol::protocol::TokenUsage;

/// Metadata about the currently running turn.
pub(crate) struct ActiveTurn {
    pub(crate) task: Option<RunningTask>,
    pub(crate) turn_state: Arc<Mutex<TurnState>>,
    taskless_start_claim: Option<Arc<TasklessTurnClaim>>,
    pending_wake_start: Option<PendingWakeStart>,
}

struct PendingWakeStart {
    reservation: Option<AgentExecutionReservation>,
    committed: bool,
}

/// Whether mailbox deliveries should still be folded into the current turn.
///
/// State machine:
/// - A turn starts in `CurrentTurn`, so queued child mail can join the next
///   model request for that turn.
/// - After user-visible terminal output is recorded, we switch to `NextTurn`
///   to leave late child mail queued instead of extending an already shown
///   answer.
/// - If the same task later gets explicit same-turn work again (a steered user
///   prompt or a tool call after an untagged preamble), we reopen `CurrentTurn`
///   so that pending child mail is drained into that follow-up request.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum MailboxDeliveryPhase {
    /// Incoming mailbox messages can still be consumed by the current turn.
    #[default]
    CurrentTurn,
    /// The current turn already emitted visible final answer text; mailbox
    /// messages should remain queued for a later turn.
    NextTurn,
}

impl Default for ActiveTurn {
    fn default() -> Self {
        Self {
            task: None,
            turn_state: Arc::new(Mutex::new(TurnState::default())),
            taskless_start_claim: None,
            pending_wake_start: None,
        }
    }
}

impl ActiveTurn {
    pub(crate) fn with_taskless_start_claim(claim: Arc<TasklessTurnClaim>) -> Self {
        Self {
            taskless_start_claim: Some(claim),
            ..Self::default()
        }
    }

    pub(crate) fn with_pending_wake_reservation(
        reservation: AgentExecutionReservation,
        claim: Arc<TasklessTurnClaim>,
    ) -> Self {
        Self {
            taskless_start_claim: Some(claim),
            pending_wake_start: Some(PendingWakeStart {
                reservation: Some(reservation),
                committed: false,
            }),
            ..Self::default()
        }
    }

    pub(crate) fn taskless_start_claim(&self) -> Option<Arc<TasklessTurnClaim>> {
        self.taskless_start_claim.as_ref().map(Arc::clone)
    }

    pub(crate) fn ensure_taskless_start_claim(&mut self, claim: Arc<TasklessTurnClaim>) {
        self.taskless_start_claim.get_or_insert(claim);
    }

    pub(crate) fn take_pending_wake_reservation_for_user(
        &mut self,
    ) -> Option<AgentExecutionReservation> {
        let pending = self.pending_wake_start.as_ref()?;
        if pending.committed {
            return None;
        }
        self.pending_wake_start
            .take()
            .and_then(|mut pending| pending.reservation.take())
    }

    pub(crate) fn commit_pending_wake(&mut self) -> Option<AgentExecutionReservation> {
        let pending = self.pending_wake_start.as_mut()?;
        if pending.committed {
            return None;
        }
        pending.committed = true;
        pending.reservation.take()
    }

    pub(crate) fn finish_taskless_start(&mut self) {
        self.pending_wake_start = None;
        self.taskless_start_claim = None;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TaskKind {
    Regular,
    Review,
    Compact,
}

pub(crate) struct RunningTask {
    pub(crate) done: Arc<Notify>,
    pub(crate) kind: TaskKind,
    pub(crate) task: Arc<dyn AnySessionTask>,
    pub(crate) cancellation_token: CancellationToken,
    pub(crate) handle: AbortOnDropHandle<()>,
    pub(crate) turn_context: Arc<TurnContext>,
    pub(crate) _agent_execution_guard: Option<AgentExecutionGuard>,
    pub(crate) _diagnostics_guard: GaugeGuard,
    // Timer recorded when the task drops to capture the full turn duration.
    pub(crate) _timer: Option<codex_otel::Timer>,
}

/// Mutable state for a single turn.
#[derive(Default)]
pub(crate) struct TurnState {
    pending_approvals: HashMap<String, oneshot::Sender<ReviewDecision>>,
    pending_request_permissions: HashMap<String, PendingRequestPermissions>,
    pending_user_input: HashMap<String, oneshot::Sender<RequestUserInputResponse>>,
    pending_elicitations: HashMap<(String, RequestId), oneshot::Sender<ElicitationResponse>>,
    mcp_tool_approval_metadata: HashMap<String, (Option<McpInvocation>, McpToolApprovalMetadata)>,
    pending_dynamic_tools: HashMap<String, oneshot::Sender<DynamicToolResponse>>,
    pub(crate) pending_input: TurnInputQueue,
    mailbox_delivery_phase: MailboxDeliveryPhase,
    granted_permissions_by_environment_id: HashMap<String, AdditionalPermissionProfile>,
    strict_auto_review_enabled: bool,
    pub(crate) tool_calls: u64,
    pub(crate) has_memory_citation: bool,
    pub(crate) token_usage_at_turn_start: TokenUsage,
    /// The last step captured for execution or selected from a speculative fallback.
    /// Remains absent until a step is captured; standalone local compaction has no step.
    pub(crate) last_known_step_context: Option<Arc<StepContext>>,
}

pub(crate) struct PendingRequestPermissions {
    pub(crate) tx_response: oneshot::Sender<RequestPermissionsResponse>,
    pub(crate) requested_permissions: RequestPermissionProfile,
    pub(crate) environment: TurnEnvironmentSelection,
}

impl TurnState {
    pub(crate) fn insert_pending_approval(
        &mut self,
        key: String,
        tx: oneshot::Sender<ReviewDecision>,
    ) -> Option<oneshot::Sender<ReviewDecision>> {
        self.pending_approvals.insert(key, tx)
    }

    pub(crate) fn remove_pending_approval(
        &mut self,
        key: &str,
    ) -> Option<oneshot::Sender<ReviewDecision>> {
        self.pending_approvals.remove(key)
    }

    pub(crate) fn clear_pending_waiters(&mut self) {
        self.pending_approvals.clear();
        self.pending_request_permissions.clear();
        self.pending_user_input.clear();
        self.pending_elicitations.clear();
        self.mcp_tool_approval_metadata.clear();
        self.pending_dynamic_tools.clear();
    }

    pub(crate) fn insert_pending_request_permissions(
        &mut self,
        key: String,
        pending_request_permissions: PendingRequestPermissions,
    ) -> Option<PendingRequestPermissions> {
        self.pending_request_permissions
            .insert(key, pending_request_permissions)
    }

    pub(crate) fn remove_pending_request_permissions(
        &mut self,
        key: &str,
    ) -> Option<PendingRequestPermissions> {
        self.pending_request_permissions.remove(key)
    }

    pub(crate) fn insert_pending_user_input(
        &mut self,
        key: String,
        tx: oneshot::Sender<RequestUserInputResponse>,
    ) -> Option<oneshot::Sender<RequestUserInputResponse>> {
        self.pending_user_input.insert(key, tx)
    }

    pub(crate) fn remove_pending_user_input(
        &mut self,
        key: &str,
    ) -> Option<oneshot::Sender<RequestUserInputResponse>> {
        self.pending_user_input.remove(key)
    }

    pub(crate) fn insert_pending_elicitation(
        &mut self,
        server_name: String,
        request_id: RequestId,
        tx: oneshot::Sender<ElicitationResponse>,
    ) -> Option<oneshot::Sender<ElicitationResponse>> {
        self.pending_elicitations
            .insert((server_name, request_id), tx)
    }

    pub(crate) fn remove_pending_elicitation(
        &mut self,
        server_name: &str,
        request_id: &RequestId,
    ) -> Option<oneshot::Sender<ElicitationResponse>> {
        self.pending_elicitations
            .remove(&(server_name.to_string(), request_id.clone()))
    }

    pub(crate) fn insert_mcp_tool_approval_metadata(
        &mut self,
        call_id: String,
        invocation: Option<McpInvocation>,
        metadata: McpToolApprovalMetadata,
    ) {
        self.mcp_tool_approval_metadata
            .insert(call_id, (invocation, metadata));
    }

    pub(crate) fn mcp_tool_approval_metadata(
        &self,
        call_id: &str,
    ) -> Option<(Option<McpInvocation>, McpToolApprovalMetadata)> {
        self.mcp_tool_approval_metadata.get(call_id).cloned()
    }

    pub(crate) fn insert_pending_dynamic_tool(
        &mut self,
        key: String,
        tx: oneshot::Sender<DynamicToolResponse>,
    ) -> Option<oneshot::Sender<DynamicToolResponse>> {
        self.pending_dynamic_tools.insert(key, tx)
    }

    pub(crate) fn remove_pending_dynamic_tool(
        &mut self,
        key: &str,
    ) -> Option<oneshot::Sender<DynamicToolResponse>> {
        self.pending_dynamic_tools.remove(key)
    }

    pub(crate) fn accept_mailbox_delivery_for_current_turn(&mut self) {
        self.set_mailbox_delivery_phase(MailboxDeliveryPhase::CurrentTurn);
    }

    pub(crate) fn accepts_mailbox_delivery_for_current_turn(&self) -> bool {
        self.mailbox_delivery_phase == MailboxDeliveryPhase::CurrentTurn
    }

    pub(crate) fn set_mailbox_delivery_phase(&mut self, phase: MailboxDeliveryPhase) {
        self.mailbox_delivery_phase = phase;
    }

    pub(crate) fn record_granted_permissions(
        &mut self,
        environment_id: &str,
        permissions: AdditionalPermissionProfile,
    ) {
        let granted_permissions = merge_permission_profiles(
            self.granted_permissions_by_environment_id
                .get(environment_id),
            Some(&permissions),
        );
        if let Some(granted_permissions) = granted_permissions {
            self.granted_permissions_by_environment_id
                .insert(environment_id.to_string(), granted_permissions);
        }
    }

    pub(crate) fn granted_permissions(
        &self,
        environment_id: &str,
    ) -> Option<AdditionalPermissionProfile> {
        self.granted_permissions_by_environment_id
            .get(environment_id)
            .cloned()
    }

    pub(crate) fn enable_strict_auto_review(&mut self) {
        self.strict_auto_review_enabled = true;
    }

    pub(crate) fn strict_auto_review_enabled(&self) -> bool {
        self.strict_auto_review_enabled
    }
}
