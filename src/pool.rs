use crate::bundle::Bundle;
use crate::{EmbedError, engine};
use openworkers_core::{HttpRequest, HttpResponse, OperationsHandle};
use std::cell::Cell;
use std::future::Future;
use std::mem::ManuallyDrop;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::thread::ThreadId;
use tokio::sync::{Notify, mpsc, oneshot};

type Job = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()>>> + Send>;

/// The threads that run workers. Each one has a current-thread tokio runtime:
/// a JavaScript engine keeps its state on the thread that made it.
pub(crate) struct WorkerPool {
    senders: Vec<mpsc::UnboundedSender<Job>>,
    loads: Vec<Arc<AtomicUsize>>,
    next: AtomicUsize,
}

/// Whether the caller of a request is gone. The job of a request is a task of
/// its own, so a caller that drops its future does not stop it: the engine
/// reads this to start a request only while its caller waits for it.
#[derive(Clone)]
pub(crate) struct Gone(Arc<Mark>);

/// Held by the caller. Dropped with the future of the caller, it marks the
/// request as gone.
pub(crate) struct Caller(Arc<Mark>);

#[derive(Default)]
pub(crate) struct Mark {
    gone: AtomicBool,
    changed: Notify,
}

impl Caller {
    pub fn new() -> (Caller, Gone) {
        let mark = Arc::new(Mark::default());

        (Caller(Arc::clone(&mark)), Gone(mark))
    }
}

impl Gone {
    pub fn is_gone(&self) -> bool {
        self.0.gone.load(Ordering::SeqCst)
    }

    /// Resolves when the caller is gone.
    #[cfg(feature = "v8")]
    pub async fn wait(&self) {
        loop {
            let changed = self.0.changed.notified();

            if self.is_gone() {
                return;
            }

            changed.await;
        }
    }
}

impl Drop for Caller {
    fn drop(&mut self) {
        self.0.gone.store(true, Ordering::SeqCst);
        self.0.changed.notify_waiters();
    }
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
    pub fn start(threads: usize) -> Result<Self, EmbedError> {
        engine::init()?;

        Ok(Self {
            senders: (0..threads).map(spawn).collect(),
            loads: (0..threads).map(|_| Arc::default()).collect(),
            next: AtomicUsize::new(0),
        })
    }

    /// Jobs that run or wait, on all threads.
    #[cfg(all(test, feature = "v8"))]
    pub fn load(&self) -> usize {
        self.loads
            .iter()
            .map(|load| load.load(Ordering::Relaxed))
            .sum()
    }

    /// Runs `set` once on each thread of the pool, for a test that changes a
    /// setting of its own threads only.
    #[cfg(all(test, feature = "v8"))]
    pub async fn on_each_thread(&self, set: fn()) {
        for sender in &self.senders {
            let (done, ran) = oneshot::channel();
            let job: Job = Box::new(move || {
                Box::pin(async move {
                    set();
                    let _ = done.send(());
                })
            });

            sender.send(job).expect("the thread runs");
            ran.await.expect("the job ran");
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
        let (_caller, gone) = Caller::new();

        self.run(move || engine::fetch(bundle, ops, request, gone))
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

        // With every thread idle, thread 0 answers: its caches stay warm
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

        // The job drops its sender without a send when it panics. The thread
        // goes on, and its channel stays open.
        result.await.map_err(|_| {
            if self.senders[index].is_closed() {
                EmbedError::ThreadGone
            } else {
                EmbedError::Panicked
            }
        })
    }
}

fn spawn(index: usize) -> mpsc::UnboundedSender<Job> {
    let (sender, mut jobs) = mpsc::unbounded_channel::<Job>();

    std::thread::Builder::new()
        .name(format!("embed-js-{index}"))
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .on_thread_park(engine::on_park)
                .build()
                .expect("tokio runtime for a worker thread");

            WORKER.with(|worker| worker.set(true));

            runtime.block_on(async move {
                while let Some(job) = jobs.recv().await {
                    spawn_here(job());
                }
            });
        })
        .expect("worker thread");

    sender
}

thread_local! {
    /// Set on a worker thread, before its runtime runs.
    static WORKER: Cell<bool> = const { Cell::new(false) };
}

/// Run a future that is not `Send` as a task of the runtime of this worker
/// thread. Called anywhere else, it aborts the process.
///
/// The engine runs its ops as tasks of the runtime, so a request runs as one
/// too: a wake between a task and the `block_on` future is slow.
pub(crate) fn spawn_here<F>(future: F)
where
    F: Future<Output = ()> + 'static,
{
    if !WORKER.with(Cell::get) {
        abort("spawn_here outside a worker thread");
    }

    // The handle drops here, on this thread, and the output is ()
    drop(tokio::spawn(OnThisThread {
        thread: std::thread::current().id(),
        future: ManuallyDrop::new(future),
    }));
}

/// A future that stays on the thread that made it. It checks this each time it
/// is polled or dropped, and aborts the process on another thread.
struct OnThisThread<F> {
    thread: ThreadId,
    future: ManuallyDrop<F>,
}

// SAFETY: `check` runs in each poll and in drop, and aborts the process on a
// thread other than the one that made the future. So the future is never
// polled or dropped on another thread. Do not remove the checks.
unsafe impl<F> Send for OnThisThread<F> {}

impl<F> OnThisThread<F> {
    fn check(&self) {
        if std::thread::current().id() != self.thread {
            abort("a worker future left its thread");
        }
    }
}

impl<F: Future> Future for OnThisThread<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        self.check();

        // SAFETY: a projection to the future, which is never moved out
        unsafe { self.map_unchecked_mut(|this| &mut *this.future) }.poll(cx)
    }
}

impl<F> Drop for OnThisThread<F> {
    fn drop(&mut self) {
        self.check();

        // SAFETY: dropped once, here, and never used again
        unsafe { ManuallyDrop::drop(&mut self.future) };
    }
}

fn abort(reason: &str) -> ! {
    eprintln!("openworkers-embed: {reason}");
    std::process::abort();
}

#[cfg(test)]
mod tests {
    use super::{Job, WorkerPool, least_loaded};
    use crate::EmbedError;
    use std::sync::atomic::Ordering;
    use tokio::sync::oneshot;

    /// A job in flight when the pool drops is dropped on its worker thread: the
    /// thread check of OnThisThread does not abort.
    #[tokio::test]
    async fn a_pool_that_drops_drops_its_jobs_on_their_thread() {
        let pool = WorkerPool::start(1).unwrap();
        let (started, running) = oneshot::channel::<()>();
        let (held, dropped) = oneshot::channel::<()>();

        let job: Job = Box::new(move || {
            Box::pin(async move {
                let _held = held;
                let _ = started.send(());

                std::future::pending::<()>().await;
            })
        });

        pool.senders[0].send(job).unwrap();
        running.await.unwrap();
        drop(pool);

        // The sender drops with the job, on the worker thread, as the runtime drops
        assert!(dropped.await.is_err());
    }

    /// A job that panics fails its caller only: the thread serves the next job.
    #[tokio::test]
    async fn a_job_that_panics_leaves_its_thread_running() {
        let pool = WorkerPool::start(1).unwrap();
        let failed = pool.run(|| async { panic!("a job that panics") }).await;

        assert!(matches!(failed, Err(EmbedError::Panicked)), "{failed:?}");
        assert_eq!(pool.loads[0].load(Ordering::Relaxed), 0);
        assert_eq!(pool.run(|| async { 1 }).await.unwrap(), 1);
    }

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
