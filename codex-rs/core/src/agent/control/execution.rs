use super::AgentControl;
use crate::codex_thread::CodexThread;
use crate::tasks::AgentExecutionReservation;
use codex_protocol::error::CodexErr;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::protocol::MultiAgentVersion;
use codex_protocol::protocol::SessionSource;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use tokio::sync::watch;

pub(super) struct AgentExecutionLimiter {
    active: AtomicUsize,
    max_threads: OnceLock<usize>,
    release_generation: AtomicU64,
    release_tx: watch::Sender<u64>,
}

impl Default for AgentExecutionLimiter {
    fn default() -> Self {
        let (release_tx, _) = watch::channel(0);
        Self {
            active: AtomicUsize::default(),
            max_threads: OnceLock::new(),
            release_generation: AtomicU64::default(),
            release_tx,
        }
    }
}

pub(crate) struct AgentExecutionGuard {
    limiter: Arc<AgentExecutionLimiter>,
}

impl Drop for AgentExecutionGuard {
    fn drop(&mut self) {
        self.limiter.active.fetch_sub(1, Ordering::AcqRel);
        let generation = self
            .limiter
            .release_generation
            .fetch_add(1, Ordering::AcqRel)
            + 1;
        self.limiter.release_tx.send_replace(generation);
    }
}

impl AgentControl {
    pub(crate) async fn ensure_execution_capacity_for_turn_start(
        &self,
        thread: &CodexThread,
    ) -> CodexResult<()> {
        if thread.session.active_turn.lock().await.is_some() {
            return Ok(());
        }
        let config = thread.session.get_config().await;
        let multi_agent_version = thread
            .multi_agent_version()
            .unwrap_or_else(|| config.multi_agent_version_from_features());
        self.ensure_execution_capacity(multi_agent_version, &thread.session_source)
    }

    pub(crate) fn ensure_execution_capacity(
        &self,
        multi_agent_version: MultiAgentVersion,
        session_source: &SessionSource,
    ) -> CodexResult<()> {
        if !is_execution_limited(multi_agent_version, session_source) {
            return Ok(());
        }
        let max_threads = self.agent_execution_limiter.max_threads();
        if self.agent_execution_limiter.has_capacity() {
            Ok(())
        } else {
            Err(CodexErr::new(CodexErrorDetails::AgentLimitReached {
                max_threads,
            }))
        }
    }

    pub(crate) fn reserve_execution_capacity(
        &self,
        multi_agent_version: MultiAgentVersion,
        session_source: &SessionSource,
    ) -> CodexResult<AgentExecutionReservation> {
        let guard = if is_execution_limited(multi_agent_version, session_source) {
            Some(Arc::clone(&self.agent_execution_limiter).try_reserve()?)
        } else {
            None
        };
        Ok(AgentExecutionReservation::from_agent_control(guard))
    }
}

impl AgentExecutionLimiter {
    pub(crate) fn subscribe_release(&self) -> watch::Receiver<u64> {
        self.release_tx.subscribe()
    }

    pub(super) fn initialize(&self, max_threads: usize) {
        self.max_threads.get_or_init(|| max_threads);
    }

    fn max_threads(&self) -> usize {
        self.max_threads.get().copied().unwrap_or(usize::MAX)
    }

    fn has_capacity(&self) -> bool {
        self.active.load(Ordering::Acquire) < self.max_threads()
    }

    fn try_reserve(self: Arc<Self>) -> CodexResult<AgentExecutionGuard> {
        let max_threads = self.max_threads();
        let mut active = self.active.load(Ordering::Acquire);
        loop {
            if active >= max_threads {
                return Err(CodexErr::new(CodexErrorDetails::AgentLimitReached {
                    max_threads,
                }));
            }
            match self.active.compare_exchange_weak(
                active,
                active + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(AgentExecutionGuard { limiter: self }),
                Err(observed) => active = observed,
            }
        }
    }
}

fn is_execution_limited(
    multi_agent_version: MultiAgentVersion,
    session_source: &SessionSource,
) -> bool {
    multi_agent_version == MultiAgentVersion::V2
        && matches!(session_source, SessionSource::SubAgent(_))
}

#[cfg(test)]
#[path = "execution_tests.rs"]
mod tests;
