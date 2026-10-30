//! v8: isolates stay warm in a thread-local pool, which is why each worker
//! thread runs a single-threaded tokio runtime.

use super::respond;
use crate::EmbedError;
use crate::bundle::Bundle;
use openworkers_core::{Event, HttpRequest, HttpResponse, OperationsHandle, RuntimeLimits};
use openworkers_runtime_v8::{
    PinnedExecuteRequest, PinnedPoolConfig, execute_pinned, init_pinned_pool,
};
use std::sync::Arc;
use tokio::sync::Semaphore;

const OWNER: &str = "embed";

// The isolate pool is thread-local and each Embed owns its threads, so these
// names never meet another worker.
const WORKER: &str = "embed";

/// Requests in flight on one thread. Each one holds a context of the thread's
/// isolate: a request past this number would build a context that runs the
/// whole bundle again, or a new isolate.
const CONCURRENCY: usize = 8;

thread_local! {
    static PLACES: Arc<Semaphore> = Arc::new(Semaphore::new(CONCURRENCY));
}

pub(crate) fn init() {
    // One owner runs trusted code: no per-owner cap. One isolate per thread
    // has a slot and a cached context for each place, so a request never
    // finds the pool full. A context is never retired: its replacement runs
    // the bundle again, and the runtime keeps memory of each retired context.
    init_pinned_pool(PinnedPoolConfig {
        max_per_thread: 1,
        max_per_owner: None,
        max_concurrent_per_isolate: CONCURRENCY,
        max_cached_contexts: CONCURRENCY,
        overcommit: false,
        max_context_reuses: u32::MAX,
        // A stream may live as long as its client reads it, and the code is trusted
        limits: RuntimeLimits {
            max_wall_clock_time_ms: 0,
            ..RuntimeLimits::default()
        },
    });
}

pub(crate) async fn fetch(
    bundle: Bundle,
    ops: OperationsHandle,
    request: HttpRequest,
) -> Result<HttpResponse, EmbedError> {
    let (task, response) = Event::fetch(request);

    // A stream keeps its place until the client has read it
    let place = PLACES
        .with(Arc::clone)
        .acquire_owned()
        .await
        .expect("the semaphore stays open");

    let execution = execute_pinned(PinnedExecuteRequest {
        owner_id: OWNER.to_string(),
        worker_id: WORKER.to_string(),
        version: 1,
        script: bundle.script(),
        ops,
        task,
        on_warm_hit: None,
        env_updated_at: None,
        abort: None,
        on_report: None,
    });

    respond(
        async move {
            let result = execution.await;

            drop(place);

            result
        },
        response,
    )
    .await
}
