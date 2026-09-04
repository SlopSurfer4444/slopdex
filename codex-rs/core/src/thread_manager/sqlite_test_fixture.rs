use super::ThreadManager;
use super::set_thread_manager_test_mode_for_tests;
use codex_agent_graph_store::AgentGraphStore;
use codex_agent_graph_store::AgentGraphStoreFuture;
use codex_agent_graph_store::ThreadSpawnEdgeCloseOutcome;
use codex_agent_graph_store::ThreadSpawnEdgeStatus;
use codex_exec_server::EnvironmentManager;
use codex_login::CodexAuth;
use codex_model_provider_info::ModelProviderInfo;
use codex_protocol::ThreadId;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Clone, Copy)]
struct SpawnEdge {
    parent_thread_id: ThreadId,
    status: ThreadSpawnEdgeStatus,
}

/// Deterministic graph store for tests that exercise agent lifecycle rather than persistence.
#[derive(Default)]
struct InMemoryAgentGraphStore {
    edges_by_child: Mutex<HashMap<ThreadId, SpawnEdge>>,
}

impl InMemoryAgentGraphStore {
    fn edges(&self) -> std::sync::MutexGuard<'_, HashMap<ThreadId, SpawnEdge>> {
        self.edges_by_child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn matching_children(
        edges: &HashMap<ThreadId, SpawnEdge>,
        parent_thread_id: ThreadId,
        status_filter: Option<ThreadSpawnEdgeStatus>,
    ) -> Vec<ThreadId> {
        let mut children = edges
            .iter()
            .filter_map(|(child_thread_id, edge)| {
                (edge.parent_thread_id == parent_thread_id
                    && status_filter.is_none_or(|status| edge.status == status))
                .then_some(*child_thread_id)
            })
            .collect::<Vec<_>>();
        children.sort_by_key(ToString::to_string);
        children
    }
}

impl AgentGraphStore for InMemoryAgentGraphStore {
    fn upsert_thread_spawn_edge(
        &self,
        parent_thread_id: ThreadId,
        child_thread_id: ThreadId,
        status: ThreadSpawnEdgeStatus,
    ) -> AgentGraphStoreFuture<'_, ()> {
        Box::pin(async move {
            self.edges().insert(
                child_thread_id,
                SpawnEdge {
                    parent_thread_id,
                    status,
                },
            );
            Ok(())
        })
    }

    fn set_thread_spawn_edge_status(
        &self,
        child_thread_id: ThreadId,
        status: ThreadSpawnEdgeStatus,
    ) -> AgentGraphStoreFuture<'_, ()> {
        Box::pin(async move {
            if let Some(edge) = self.edges().get_mut(&child_thread_id) {
                edge.status = status;
            }
            Ok(())
        })
    }

    fn close_open_thread_spawn_edge(
        &self,
        parent_thread_id: ThreadId,
        child_thread_id: ThreadId,
    ) -> AgentGraphStoreFuture<'_, ThreadSpawnEdgeCloseOutcome> {
        Box::pin(async move {
            let mut edges = self.edges();
            let outcome = match edges.get_mut(&child_thread_id) {
                Some(edge) if edge.parent_thread_id == parent_thread_id => match edge.status {
                    ThreadSpawnEdgeStatus::Open => {
                        edge.status = ThreadSpawnEdgeStatus::Closed;
                        ThreadSpawnEdgeCloseOutcome::NewlyClosed
                    }
                    ThreadSpawnEdgeStatus::Closed => {
                        ThreadSpawnEdgeCloseOutcome::AlreadyExactClosed
                    }
                },
                Some(_) | None => ThreadSpawnEdgeCloseOutcome::MismatchOrMissing,
            };
            Ok(outcome)
        })
    }

    fn list_thread_spawn_children(
        &self,
        parent_thread_id: ThreadId,
        status_filter: Option<ThreadSpawnEdgeStatus>,
    ) -> AgentGraphStoreFuture<'_, Vec<ThreadId>> {
        Box::pin(async move {
            Ok(Self::matching_children(
                &self.edges(),
                parent_thread_id,
                status_filter,
            ))
        })
    }

    fn list_thread_spawn_descendants(
        &self,
        root_thread_id: ThreadId,
        status_filter: Option<ThreadSpawnEdgeStatus>,
    ) -> AgentGraphStoreFuture<'_, Vec<ThreadId>> {
        Box::pin(async move {
            let edges = self.edges();
            let mut seen = HashSet::from([root_thread_id]);
            let mut frontier = vec![root_thread_id];
            let mut descendants = Vec::new();

            while !frontier.is_empty() {
                let parents = frontier.into_iter().collect::<HashSet<_>>();
                let mut next = edges
                    .iter()
                    .filter_map(|(child_thread_id, edge)| {
                        (parents.contains(&edge.parent_thread_id)
                            && status_filter.is_none_or(|status| edge.status == status))
                        .then_some(*child_thread_id)
                    })
                    .collect::<Vec<_>>();
                next.sort_by_key(ToString::to_string);
                next.retain(|thread_id| seen.insert(*thread_id));
                descendants.extend(next.iter().copied());
                frontier = next;
            }

            Ok(descendants)
        })
    }
}

/// Owns every resource backing a thread manager test with an in-memory graph store.
///
/// Call [`Self::teardown`] so test threads stop before the exact fixture root is removed.
pub(crate) struct ThreadManagerTestFixture {
    manager: Option<ThreadManager>,
    codex_home: PathBuf,
}

impl Deref for ThreadManagerTestFixture {
    type Target = ThreadManager;

    fn deref(&self) -> &Self::Target {
        self.manager
            .as_ref()
            .expect("test manager must remain available until teardown")
    }
}

impl ThreadManagerTestFixture {
    pub(crate) fn codex_home(&self) -> &std::path::Path {
        &self.codex_home
    }

    pub(crate) async fn teardown(mut self) {
        let manager = self
            .manager
            .take()
            .expect("test manager must remain available until teardown");
        let report = manager
            .shutdown_all_threads_bounded(Duration::from_secs(10))
            .await;
        assert_eq!(
            report.submit_failed,
            Vec::<ThreadId>::new(),
            "all test threads must accept shutdown before fixture teardown"
        );
        assert_eq!(
            report.timed_out,
            Vec::<ThreadId>::new(),
            "all test threads must stop before fixture teardown"
        );
        drop(manager);
        tokio::fs::remove_dir_all(&self.codex_home)
            .await
            .unwrap_or_else(|err| panic!("remove {}: {err}", self.codex_home.display()));
        assert!(
            !tokio::fs::try_exists(&self.codex_home)
                .await
                .unwrap_or_else(|err| panic!("inspect {}: {err}", self.codex_home.display())),
            "test fixture root must be absent after teardown: {}",
            self.codex_home.display()
        );
    }
}

impl ThreadManager {
    /// Construct a test manager with the graph-store prerequisite required by
    /// non-ephemeral multi-agent V2 spawns, without introducing persistence.
    pub(crate) async fn with_models_provider_and_graph_store_for_tests(
        auth: CodexAuth,
        provider: ModelProviderInfo,
    ) -> ThreadManagerTestFixture {
        Self::with_models_provider_graph_store_and_environment_for_tests(
            auth,
            provider,
            Arc::new(EnvironmentManager::default_for_tests()),
        )
        .await
    }

    /// Construct the in-memory graph-store test manager with a caller-provided
    /// runtime environment manager.
    pub(crate) async fn with_models_provider_graph_store_and_environment_for_tests(
        auth: CodexAuth,
        provider: ModelProviderInfo,
        environment_manager: Arc<EnvironmentManager>,
    ) -> ThreadManagerTestFixture {
        set_thread_manager_test_mode_for_tests(/*enabled*/ true);
        let codex_home = std::env::temp_dir().join(format!(
            "codex-thread-manager-graph-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&codex_home)
            .unwrap_or_else(|err| panic!("temp codex home dir create failed: {err}"));
        let mut manager = Self::with_models_provider_home_state_and_bundled_skills_for_tests(
            auth,
            provider,
            codex_home.clone(),
            environment_manager,
            /*state_db*/ None,
            /*bundled_skills_enabled*/ false,
        );
        let state = Arc::get_mut(&mut manager.state)
            .expect("fresh test manager state should not yet be shared");
        state.agent_graph_store = Some(Arc::new(InMemoryAgentGraphStore::default()));
        ThreadManagerTestFixture {
            manager: Some(manager),
            codex_home,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn thread_id(suffix: u128) -> ThreadId {
        ThreadId::from_string(&format!("00000000-0000-0000-0000-{suffix:012}"))
            .expect("valid thread id")
    }

    #[tokio::test]
    async fn in_memory_graph_fixture_does_not_install_bundled_skills() {
        let fixture = ThreadManager::with_models_provider_and_graph_store_for_tests(
            CodexAuth::from_api_key("dummy"),
            codex_model_provider_info::built_in_model_providers(/*openai_base_url*/ None)["openai"]
                .clone(),
        )
        .await;
        let skills_root = fixture.codex_home().join("skills");

        assert!(
            !tokio::fs::try_exists(&skills_root)
                .await
                .unwrap_or_else(|err| panic!("inspect {}: {err}", skills_root.display())),
            "the general in-memory fixture must not install bundled skills: {}",
            skills_root.display()
        );
        fixture.teardown().await;
    }

    #[tokio::test]
    async fn in_memory_graph_store_upsert_status_and_close_contract() {
        let store = InMemoryAgentGraphStore::default();
        let first_parent = thread_id(1);
        let second_parent = thread_id(2);
        let child = thread_id(3);

        store
            .set_thread_spawn_edge_status(child, ThreadSpawnEdgeStatus::Closed)
            .await
            .expect("missing status update should be a no-op");
        store
            .upsert_thread_spawn_edge(first_parent, child, ThreadSpawnEdgeStatus::Open)
            .await
            .expect("initial edge should insert");
        store
            .upsert_thread_spawn_edge(second_parent, child, ThreadSpawnEdgeStatus::Open)
            .await
            .expect("upsert should replace the child's parent and status");
        store
            .set_thread_spawn_edge_status(child, ThreadSpawnEdgeStatus::Closed)
            .await
            .expect("existing status should update");

        assert_eq!(
            store
                .list_thread_spawn_children(first_parent, None)
                .await
                .expect("old parent should list"),
            Vec::<ThreadId>::new()
        );
        assert_eq!(
            store
                .list_thread_spawn_children(second_parent, Some(ThreadSpawnEdgeStatus::Closed),)
                .await
                .expect("updated status should be observable"),
            vec![child]
        );
        store
            .set_thread_spawn_edge_status(child, ThreadSpawnEdgeStatus::Open)
            .await
            .expect("existing status should update again");
        assert_eq!(
            store
                .close_open_thread_spawn_edge(first_parent, child)
                .await
                .expect("mismatched close should resolve"),
            ThreadSpawnEdgeCloseOutcome::MismatchOrMissing
        );
        assert_eq!(
            store
                .close_open_thread_spawn_edge(second_parent, child)
                .await
                .expect("exact open edge should close"),
            ThreadSpawnEdgeCloseOutcome::NewlyClosed
        );
        assert_eq!(
            store
                .close_open_thread_spawn_edge(second_parent, child)
                .await
                .expect("exact closed edge should confirm"),
            ThreadSpawnEdgeCloseOutcome::AlreadyExactClosed
        );
        assert_eq!(
            store
                .close_open_thread_spawn_edge(second_parent, thread_id(99))
                .await
                .expect("missing edge close should resolve"),
            ThreadSpawnEdgeCloseOutcome::MismatchOrMissing
        );
    }

    #[tokio::test]
    async fn in_memory_graph_store_lists_children_and_descendants_in_contract_order() {
        let store = InMemoryAgentGraphStore::default();
        let root = thread_id(10);
        let child_a = thread_id(20);
        let child_b = thread_id(30);
        let child_closed = thread_id(40);
        let grandchild_a = thread_id(50);
        let grandchild_closed = thread_id(60);
        let hidden_below_closed = thread_id(70);
        for (parent, child, status) in [
            (root, child_b, ThreadSpawnEdgeStatus::Open),
            (
                child_closed,
                hidden_below_closed,
                ThreadSpawnEdgeStatus::Open,
            ),
            (root, child_a, ThreadSpawnEdgeStatus::Open),
            (child_b, grandchild_closed, ThreadSpawnEdgeStatus::Closed),
            (root, child_closed, ThreadSpawnEdgeStatus::Closed),
            (child_a, grandchild_a, ThreadSpawnEdgeStatus::Open),
            (hidden_below_closed, root, ThreadSpawnEdgeStatus::Open),
        ] {
            store
                .upsert_thread_spawn_edge(parent, child, status)
                .await
                .expect("edge should insert");
        }

        assert_eq!(
            store
                .list_thread_spawn_children(root, None)
                .await
                .expect("direct children should list"),
            vec![child_a, child_b, child_closed]
        );
        assert_eq!(
            store
                .list_thread_spawn_children(root, Some(ThreadSpawnEdgeStatus::Open))
                .await
                .expect("open direct children should list"),
            vec![child_a, child_b]
        );
        assert_eq!(
            store
                .list_thread_spawn_children(root, Some(ThreadSpawnEdgeStatus::Closed))
                .await
                .expect("closed direct children should list"),
            vec![child_closed]
        );
        assert_eq!(
            store
                .list_thread_spawn_descendants(root, None)
                .await
                .expect("all descendants should list"),
            vec![
                child_a,
                child_b,
                child_closed,
                grandchild_a,
                grandchild_closed,
                hidden_below_closed,
            ]
        );
        assert_eq!(
            store
                .list_thread_spawn_descendants(root, Some(ThreadSpawnEdgeStatus::Open))
                .await
                .expect("open descendants should list"),
            vec![child_a, child_b, grandchild_a]
        );
        assert_eq!(
            store
                .list_thread_spawn_descendants(root, Some(ThreadSpawnEdgeStatus::Closed))
                .await
                .expect("closed descendants should list"),
            vec![child_closed]
        );
    }
}
