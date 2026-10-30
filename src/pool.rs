use crate::bundle::Bundle;
use crate::{EmbedError, engine};
use openworkers_core::{HttpRequest, HttpResponse, OperationsHandle};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{mpsc, oneshot};

type Job = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()>>> + Send>;

/// The threads that run workers. Each one has a single-threaded tokio runtime:
/// a JavaScript engine keeps its state on the thread that made it.
pub(crate) struct WorkerPool {
    senders: Vec<mpsc::UnboundedSender<Job>>,
    next: AtomicUsize,
}

impl WorkerPool {
    pub fn start(threads: usize) -> Self {
        engine::init();

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

        self.run(move || engine::fetch(bundle, ops, request))
            .await?
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

fn spawn(index: usize) -> mpsc::UnboundedSender<Job> {
    let (sender, mut jobs) = mpsc::unbounded_channel::<Job>();

    std::thread::Builder::new()
        .name(format!("embed-js-{index}"))
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
