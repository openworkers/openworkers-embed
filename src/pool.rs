use crate::bundle::Bundle;
use crate::{EmbedError, engine};
use openworkers_core::{HttpRequest, HttpResponse, OperationsHandle};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{mpsc, oneshot};

type Job = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()>>> + Send>;

/// The threads that run workers. Each one has a single-threaded tokio runtime:
/// a JavaScript engine keeps its state on the thread that made it.
pub(crate) struct WorkerPool {
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

        Self {
            senders: (0..threads).map(spawn).collect(),
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

    async fn run<F, Fut, T>(&self, job: F) -> Result<T, EmbedError>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = T> + 'static,
        T: Send + 'static,
    {
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
