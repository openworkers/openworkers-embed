use crate::bundle::Bundle;
use crate::{EmbedError, engine};
use openworkers_core::{HttpRequest, HttpResponse, OperationsHandle};
use std::cell::Cell;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{mpsc, oneshot};

type Job = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()>>> + Send>;

static POOLS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// The pool and the index of the worker thread this is, if it is one.
    static WORKER: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
}

/// The threads that run workers. Each one has a single-threaded tokio runtime:
/// a JavaScript engine keeps its state on the thread that made it.
pub(crate) struct WorkerPool {
    id: usize,
    senders: Vec<mpsc::UnboundedSender<Job>>,
    loads: Vec<Arc<AtomicUsize>>,
    next: AtomicUsize,
}

/// A job sent to a thread and not finished: counted until it ends or is dropped.
struct InFlight(Arc<AtomicUsize>);

impl InFlight {
    fn start(load: &Arc<AtomicUsize>) -> Self {
        load.fetch_add(1, Ordering::Relaxed);

        Self(Arc::clone(load))
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// The thread with the fewest jobs, looking from `start` so that ties spread.
fn least_loaded(loads: &[usize], start: usize) -> usize {
    (0..loads.len())
        .map(|offset| (start + offset) % loads.len())
        .min_by_key(|&index| loads[index])
        .expect("a pool has a thread")
}

impl WorkerPool {
    pub fn start(threads: usize) -> Self {
        engine::init();

        let id = POOLS.fetch_add(1, Ordering::Relaxed);

        Self {
            id,
            senders: (0..threads).map(|index| spawn(id, index)).collect(),
            loads: (0..threads).map(|_| Arc::default()).collect(),
            next: AtomicUsize::new(0),
        }
    }

    /// Run a bundle for one request on the thread with the fewest requests.
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

    /// The number of worker threads.
    #[cfg(feature = "axum")]
    pub fn threads(&self) -> usize {
        self.senders.len()
    }

    /// Run `job` on the worker thread `index`, for as long as it lasts.
    #[cfg(feature = "axum")]
    pub fn spawn_on<F, Fut>(&self, index: usize, job: F) -> Result<(), EmbedError>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + 'static,
    {
        let job: Job = Box::new(move || Box::pin(job()));

        self.senders[index]
            .send(job)
            .map_err(|_| EmbedError::ThreadGone)
    }

    async fn run<F, Fut, T>(&self, job: F) -> Result<T, EmbedError>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = T> + 'static,
        T: Send + 'static,
    {
        // A request that arrives on a worker thread runs there: a server on
        // the worker threads pays no hop to another thread
        if let Some((pool, index)) = WORKER.get()
            && pool == self.id
        {
            let in_flight = InFlight::start(&self.loads[index]);
            let (done, result) = oneshot::channel();

            tokio::task::spawn_local(async move {
                let _ = done.send(job().await);

                drop(in_flight);
            });

            return result.await.map_err(|_| EmbedError::ThreadGone);
        }

        let loads: Vec<usize> = self
            .loads
            .iter()
            .map(|load| load.load(Ordering::Relaxed))
            .collect();
        let rotating = self.next.fetch_add(1, Ordering::Relaxed);
        let idle = loads.iter().all(|&load| load == 0);
        let index = least_loaded(&loads, if idle { 0 } else { rotating });
        let in_flight = InFlight::start(&self.loads[index]);
        let (done, result) = oneshot::channel();

        let job: Job = Box::new(move || {
            Box::pin(async move {
                let _ = done.send(job().await);

                drop(in_flight);
            })
        });

        self.senders[index]
            .send(job)
            .map_err(|_| EmbedError::ThreadGone)?;

        result.await.map_err(|_| EmbedError::ThreadGone)
    }
}

fn spawn(pool: usize, index: usize) -> mpsc::UnboundedSender<Job> {
    let (sender, mut jobs) = mpsc::unbounded_channel::<Job>();

    std::thread::Builder::new()
        .name(format!("embed-js-{index}"))
        .spawn(move || {
            WORKER.set(Some((pool, index)));

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

#[cfg(test)]
mod tests {
    use super::least_loaded;

    #[test]
    fn the_least_loaded_thread_is_chosen() {
        assert_eq!(least_loaded(&[3, 1, 2], 0), 1);
        assert_eq!(least_loaded(&[3, 1, 2], 2), 1);
    }

    #[test]
    fn ties_go_to_the_thread_after_the_start() {
        assert_eq!(least_loaded(&[0, 0, 0], 0), 0);
        assert_eq!(least_loaded(&[0, 0, 0], 1), 1);
        assert_eq!(least_loaded(&[0, 0, 0], 5), 2);
        assert_eq!(least_loaded(&[2, 0, 5, 0], 2), 3);
    }
}
