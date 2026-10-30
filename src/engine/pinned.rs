//! v8: isolates stay warm in a thread-local pool, which is why each worker
//! thread runs a single-threaded tokio runtime.

use super::buffered;
use crate::EmbedError;
use crate::bundle::Bundle;
use openworkers_core::{Event, HttpRequest, HttpResponse, OperationsHandle, RuntimeLimits};
use openworkers_runtime_v8::{
    PinnedExecuteRequest, PinnedPoolConfig, execute_pinned, init_pinned_pool,
};

const OWNER: &str = "embed";

// The isolate pool is thread-local and each Embed owns its threads, so these
// names never meet another worker.
const WORKER: &str = "embed";

pub(crate) fn init() {
    // One owner runs trusted code: no per-owner cap
    init_pinned_pool(PinnedPoolConfig {
        max_per_thread: 4,
        max_per_owner: None,
        max_concurrent_per_isolate: 8,
        max_cached_contexts: 4,
        overcommit: true,
        max_context_reuses: 1000,
        limits: RuntimeLimits::default(),
    });
}

pub(crate) async fn fetch(
    bundle: Bundle,
    ops: OperationsHandle,
    request: HttpRequest,
) -> Result<HttpResponse, EmbedError> {
    let (task, response) = Event::fetch(request);

    execute_pinned(PinnedExecuteRequest {
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
    })
    .await
    .map_err(EmbedError::Worker)?;

    buffered(response).await
}
