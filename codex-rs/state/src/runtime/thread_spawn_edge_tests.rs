use super::test_fixture::StateRuntimeTestFixture;
use crate::DirectionalThreadSpawnEdgeStatus;
use crate::ThreadSpawnEdgeCloseOutcome;
use codex_protocol::ThreadId;
use std::future::Future;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;

#[tokio::test]
async fn thread_spawn_edge_mutations_share_the_runtime_fence() {
    let runtime = StateRuntimeTestFixture::new().await;
    let parent_thread_id = ThreadId::new();
    let child_thread_id = ThreadId::new();

    let fence = runtime.acquire_thread_spawn_edge_fence().await;
    let upsert_runtime = Arc::clone(&runtime);
    let upsert = tokio::spawn(async move {
        upsert_runtime
            .upsert_thread_spawn_edge(
                parent_thread_id,
                child_thread_id,
                DirectionalThreadSpawnEdgeStatus::Open,
            )
            .await
    });
    tokio::task::yield_now().await;
    assert!(!upsert.is_finished());
    drop(fence);
    upsert
        .await
        .expect("upsert task should join")
        .expect("upsert should succeed");

    let fence = runtime.acquire_thread_spawn_edge_fence().await;
    let status_runtime = Arc::clone(&runtime);
    let status = tokio::spawn(async move {
        status_runtime
            .set_thread_spawn_edge_status(child_thread_id, DirectionalThreadSpawnEdgeStatus::Closed)
            .await
    });
    tokio::task::yield_now().await;
    assert!(!status.is_finished());
    drop(fence);
    status
        .await
        .expect("status task should join")
        .expect("status update should succeed");
    assert_eq!(
        runtime
            .list_thread_spawn_children_with_status(
                parent_thread_id,
                DirectionalThreadSpawnEdgeStatus::Closed,
            )
            .await
            .expect("closed children should load"),
        vec![child_thread_id]
    );

    let fence = runtime.acquire_thread_spawn_edge_fence().await;
    let delete_runtime = Arc::clone(&runtime);
    let delete = tokio::spawn(async move {
        delete_runtime
            .delete_threads_strict(&[parent_thread_id])
            .await
    });
    tokio::task::yield_now().await;
    assert!(!delete.is_finished());
    drop(fence);
    delete
        .await
        .expect("delete task should join")
        .expect("delete should succeed");
    assert_eq!(
        runtime
            .list_thread_spawn_children(parent_thread_id)
            .await
            .expect("remaining children should load"),
        Vec::<ThreadId>::new()
    );
    runtime.teardown().await;
}

#[tokio::test]
async fn close_open_thread_spawn_edge_confirms_the_exact_closed_edge() {
    let runtime = StateRuntimeTestFixture::new().await;
    let parent_thread_id = ThreadId::new();
    let other_parent_thread_id = ThreadId::new();
    let child_thread_id = ThreadId::new();
    runtime
        .upsert_thread_spawn_edge(
            parent_thread_id,
            child_thread_id,
            DirectionalThreadSpawnEdgeStatus::Open,
        )
        .await
        .expect("Open child edge should seed");

    assert_eq!(
        runtime
            .close_open_thread_spawn_edge(other_parent_thread_id, child_thread_id)
            .await
            .expect("non-matching parent should not change the edge"),
        ThreadSpawnEdgeCloseOutcome::MismatchOrMissing
    );
    assert_eq!(
        runtime
            .close_open_thread_spawn_edge(parent_thread_id, child_thread_id)
            .await
            .expect("exact Open edge should close"),
        ThreadSpawnEdgeCloseOutcome::NewlyClosed
    );
    assert_eq!(
        runtime
            .close_open_thread_spawn_edge(parent_thread_id, child_thread_id)
            .await
            .expect("exact already Closed edge should be confirmed"),
        ThreadSpawnEdgeCloseOutcome::AlreadyExactClosed
    );

    assert_eq!(
        runtime
            .list_thread_spawn_children_with_status(
                parent_thread_id,
                DirectionalThreadSpawnEdgeStatus::Closed,
            )
            .await
            .expect("Closed child edge should load"),
        vec![child_thread_id]
    );
    runtime.teardown().await;
}

#[tokio::test]
async fn close_open_thread_spawn_edge_distinguishes_new_close_from_idempotent_confirmation() {
    let runtime = StateRuntimeTestFixture::new().await;
    let parent_thread_id = ThreadId::new();
    let other_parent_thread_id = ThreadId::new();
    let child_thread_id = ThreadId::new();
    runtime
        .upsert_thread_spawn_edge(
            parent_thread_id,
            child_thread_id,
            DirectionalThreadSpawnEdgeStatus::Open,
        )
        .await
        .expect("Open child edge should seed");

    let mismatch_or_missing = runtime
        .close_open_thread_spawn_edge(other_parent_thread_id, child_thread_id)
        .await
        .expect("missing exact Open edge should remain negative");
    let first_close = runtime
        .close_open_thread_spawn_edge(parent_thread_id, child_thread_id)
        .await
        .expect("exact Open edge should close");
    let idempotent_confirmation = runtime
        .close_open_thread_spawn_edge(parent_thread_id, child_thread_id)
        .await
        .expect("exact already Closed edge should be confirmed");

    assert_eq!(
        (first_close, idempotent_confirmation, mismatch_or_missing),
        (
            ThreadSpawnEdgeCloseOutcome::NewlyClosed,
            ThreadSpawnEdgeCloseOutcome::AlreadyExactClosed,
            ThreadSpawnEdgeCloseOutcome::MismatchOrMissing,
        )
    );
    runtime.teardown().await;
}

#[tokio::test]
async fn thread_spawn_edge_close_cancellation_keeps_fence_until_sqlite_worker_drains() {
    let runtime = StateRuntimeTestFixture::new().await;
    let parent_thread_id = ThreadId::new();
    let child_thread_id = ThreadId::new();
    runtime
        .upsert_thread_spawn_edge(
            parent_thread_id,
            child_thread_id,
            DirectionalThreadSpawnEdgeStatus::Open,
        )
        .await
        .expect("Open child edge should seed");

    let mut held_connections = Vec::with_capacity(4);
    for _ in 0..4 {
        held_connections.push(
            runtime
                .pool
                .acquire()
                .await
                .expect("pool connection should acquire"),
        );
    }
    let mut instrumented_connection = runtime
        .pool
        .acquire()
        .await
        .expect("fifth pool connection should acquire");
    let entered = Arc::new((Mutex::new(false), Condvar::new()));
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let entered_callback = Arc::clone(&entered);
    let release_callback = Arc::clone(&release);
    let mut locked_handle = instrumented_connection
        .lock_handle()
        .await
        .expect("instrumented connection handle should lock");
    locked_handle.set_progress_handler(1, move || {
        let (entered_lock, entered_condvar) = &*entered_callback;
        let mut has_entered = entered_lock.lock().expect("entered signal should lock");
        if !*has_entered {
            *has_entered = true;
            entered_condvar.notify_one();
        }
        drop(has_entered);

        let (release_lock, release_condvar) = &*release_callback;
        let mut is_released = release_lock.lock().expect("release signal should lock");
        while !*is_released {
            is_released = release_condvar
                .wait(is_released)
                .expect("release signal should remain valid");
        }
        true
    });
    drop(locked_handle);
    drop(instrumented_connection);

    let entered_wait = {
        let entered = Arc::clone(&entered);
        tokio::task::spawn_blocking(move || {
            let (entered_lock, entered_condvar) = &*entered;
            let mut has_entered = entered_lock.lock().expect("entered signal should lock");
            while !*has_entered {
                has_entered = entered_condvar
                    .wait(has_entered)
                    .expect("entered signal should remain valid");
            }
        })
    };
    let close_runtime = Arc::clone(&runtime);
    let close = tokio::spawn(async move {
        close_runtime
            .close_open_thread_spawn_edge(parent_thread_id, child_thread_id)
            .await
    });
    entered_wait
        .await
        .expect("entered waiter should join once the SQLite worker is blocked");

    close.abort();
    assert!(
        close
            .await
            .expect_err("cancelled close caller should not join")
            .is_cancelled(),
        "close caller should report cancellation"
    );

    let fence_stayed_held = {
        let fence_acquire = runtime.acquire_thread_spawn_edge_fence();
        let mut fence_acquire = std::pin::pin!(fence_acquire);
        let mut context = Context::from_waker(Waker::noop());
        matches!(fence_acquire.as_mut().poll(&mut context), Poll::Pending)
    };

    {
        let (release_lock, release_condvar) = &*release;
        let mut is_released = release_lock.lock().expect("release signal should lock");
        *is_released = true;
        release_condvar.notify_one();
    }

    let mut drain_connection = runtime
        .pool
        .acquire()
        .await
        .expect("instrumented connection should return to the pool");
    let status: String = sqlx::query_scalar(
        "SELECT status FROM thread_spawn_edges WHERE parent_thread_id = ? AND child_thread_id = ?",
    )
    .bind(parent_thread_id.to_string())
    .bind(child_thread_id.to_string())
    .fetch_one(&mut *drain_connection)
    .await
    .expect("serialized SQLite worker drain should read the exact edge");
    assert_eq!(status, "closed");
    let mut locked_handle = drain_connection
        .lock_handle()
        .await
        .expect("drained connection handle should lock");
    locked_handle.remove_progress_handler();
    drop(locked_handle);
    drop(drain_connection);
    drop(held_connections);

    assert!(
        fence_stayed_held,
        "cancelling the close caller must keep the same runtime fence Pending until its SQLite worker drains"
    );
    runtime.teardown().await;
}

#[tokio::test]
async fn close_open_thread_spawn_edge_is_scoped_to_its_runtime() {
    let runtime = StateRuntimeTestFixture::new().await;
    let other_runtime = StateRuntimeTestFixture::new().await;
    let parent_thread_id = ThreadId::new();
    let child_thread_id = ThreadId::new();
    runtime
        .upsert_thread_spawn_edge(
            parent_thread_id,
            child_thread_id,
            DirectionalThreadSpawnEdgeStatus::Open,
        )
        .await
        .expect("Open child edge should seed");
    other_runtime
        .upsert_thread_spawn_edge(
            parent_thread_id,
            child_thread_id,
            DirectionalThreadSpawnEdgeStatus::Open,
        )
        .await
        .expect("other runtime Open child edge should seed");

    assert_eq!(
        runtime
            .close_open_thread_spawn_edge(parent_thread_id, child_thread_id)
            .await
            .expect("exact runtime edge should close"),
        ThreadSpawnEdgeCloseOutcome::NewlyClosed
    );

    assert_eq!(
        other_runtime
            .list_thread_spawn_children_with_status(
                parent_thread_id,
                DirectionalThreadSpawnEdgeStatus::Open,
            )
            .await
            .expect("other runtime Open child edge should remain"),
        vec![child_thread_id]
    );
    runtime.teardown().await;
    other_runtime.teardown().await;
}
