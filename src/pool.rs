use crate::EmbedError;
use crate::bundle::Bundle;
use openworkers_core::{
    Event, HttpRequest, HttpResponse, OperationsHandle, ResponseBody, RuntimeLimits,
};
use openworkers_runtime_v8::{
    PinnedExecuteRequest, PinnedPoolConfig, execute_pinned, init_pinned_pool,
};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{mpsc, oneshot};

// The isolate pool is thread-local and each Embed owns its threads, so these
// names never meet another worker.
const OWNER: &str = "embed";
const WORKER: &str = "embed";

type Job = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()>>> + Send>;

/// The threads that run workers. Each one has a single-threaded tokio runtime,
/// because the isolate pool of `openworkers-runtime-v8` is thread-local.
pub(crate) struct WorkerPool {
    senders: Vec<mpsc::UnboundedSender<Job>>,
    next: AtomicUsize,
}

impl WorkerPool {
    pub fn start(threads: usize) -> Self {
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

        Self {
            senders: (0..threads).map(spawn).collect(),
            next: AtomicUsize::new(0),
        }
    }

    /// Run a bundle for one request on the next thread in turn.
    pub async fn execute(
        &self,
        bundle: &Bundle,
        ops: OperationsHandle,
        request: HttpRequest,
    ) -> Result<HttpResponse, EmbedError> {
        let bundle = bundle.clone();

        self.run(move || fetch(bundle, ops, request)).await?
    }

    async fn run<F, Fut, T>(&self, job: F) -> Result<T, EmbedError>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = T> + 'static,
        T: Send + 'static,
    {
        let index = self.next.fetch_add(1, Ordering::Relaxed) % self.senders.len();
        let (done, result) = oneshot::channel();

        let job: Job = Box::new(move || {
            Box::pin(async move {
                let _ = done.send(job().await);
            })
        });

        self.senders[index]
            .send(job)
            .map_err(|_| EmbedError::ThreadGone)?;

        result.await.map_err(|_| EmbedError::ThreadGone)
    }
}

async fn fetch(
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

    let response = response.await.map_err(|_| EmbedError::NoResponse)?;
    let body = response
        .body
        .collect()
        .await
        .map_err(EmbedError::Body)?
        .unwrap_or_default();

    Ok(HttpResponse {
        status: response.status,
        headers: response.headers,
        body: ResponseBody::Bytes(body),
    })
}

fn spawn(index: usize) -> mpsc::UnboundedSender<Job> {
    let (sender, mut jobs) = mpsc::unbounded_channel::<Job>();

    std::thread::Builder::new()
        .name(format!("embed-v8-{index}"))
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime for a worker thread");
            let local = tokio::task::LocalSet::new();

            local.block_on(&runtime, async move {
                while let Some(job) = jobs.recv().await {
                    tokio::task::spawn_local(job());
                }
            });
        })
        .expect("worker thread");

    sender
}
