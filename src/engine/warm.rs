//! v8: each worker thread keeps warm places, an isolate with the one context
//! that runs the bundle. A request takes a place, resets its context and
//! gives it back, so the bundle runs once per place.

use super::respond;
use crate::EmbedError;
use crate::bundle::Bundle;
use crate::pool::Gone;
use openworkers_core::{
    Event, HttpRequest, HttpResponse, OperationsHandle, RuntimeLimits, TerminationReason,
};
use openworkers_runtime_v8::{ExecutionContext, LockerManagedIsolate};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{Semaphore, oneshot};

/// Requests in flight on one thread. A stream keeps its place until the
/// client has read it, so this is also the number of streams a thread serves.
const PLACES: usize = 8;

struct Place {
    isolate: Arc<LockerManagedIsolate>,
    context: ExecutionContext,
}

/// The places per thread: `INV_PLACES` if it is set, else `PLACES`.
static SETTING: OnceLock<usize> = OnceLock::new();

/// Read `INV_PLACES` before the worker threads start. The first Embed of the
/// process sets the value; a later Embed with another value fails. Starts
/// V8 too (ICU data, flags, platform), once per process, so its cost and
/// its failures come at the build and not on the first request.
pub(crate) fn init() -> Result<(), EmbedError> {
    openworkers_runtime_v8::platform::get_platform();

    let places = places(setting(std::env::var("INV_PLACES"))?.as_deref())?;

    same(*SETTING.get_or_init(|| places), places)
}

/// The text of the variable; not UTF-8 is an error, not an unset variable.
fn setting(var: Result<String, std::env::VarError>) -> Result<Option<String>, EmbedError> {
    match var {
        Ok(setting) => Ok(Some(setting)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(EmbedError::Setting("INV_PLACES is not UTF-8".to_string()))
        }
    }
}

/// The value of the first Embed must hold for the later ones.
fn same(first: usize, now: usize) -> Result<(), EmbedError> {
    if first == now {
        return Ok(());
    }

    Err(EmbedError::Setting(
        "INV_PLACES changed after the first Embed of the process".to_string(),
    ))
}

/// A count of places from 1 to the limit of a semaphore. A bad value is an
/// error: 0 would hold every request for ever.
fn places(setting: Option<&str>) -> Result<usize, EmbedError> {
    let Some(setting) = setting else {
        return Ok(PLACES);
    };

    match setting.trim().parse::<usize>() {
        Ok(places) if (1..=Semaphore::MAX_PERMITS).contains(&places) => Ok(places),
        _ => Err(EmbedError::Setting(format!(
            "INV_PLACES must be a number from 1 to {}, not {setting:?}",
            Semaphore::MAX_PERMITS
        ))),
    }
}

/// The longest time a request waits while the thread makes no progress.
const MAX_HOLD: Duration = Duration::from_millis(10);

#[cfg(test)]
thread_local! {
    /// `MAX_HOLD` on this thread, which a test can make longer for the
    /// threads of its own pool.
    static HOLD: Cell<Duration> = const { Cell::new(MAX_HOLD) };
}

#[cfg(test)]
fn max_hold() -> Duration {
    HOLD.get()
}

#[cfg(not(test))]
fn max_hold() -> Duration {
    MAX_HOLD
}

thread_local! {
    static FREE: RefCell<Vec<Place>> = const { RefCell::new(Vec::new()) };
    static PERMITS: Arc<Semaphore> = Arc::new(Semaphore::new(
        *SETTING.get().expect("init runs before the worker threads"),
    ));

    /// Requests that run, from their start to the end of their waitUntil work.
    static ACTIVE: Cell<usize> = const { Cell::new(0) };

    /// Requests that hold a permit and wait to start, oldest first.
    static WAITING: RefCell<VecDeque<oneshot::Sender<()>>> =
        const { RefCell::new(VecDeque::new()) };

    /// The last time a request ended or started, or the thread parked.
    static PROGRESS: Cell<Instant> = Cell::new(Instant::now());
}

/// A request that runs; its drop starts the oldest waiting one.
struct Running;

/// Wait for the turn of this request on its thread, in order of arrival.
/// `None` if the caller is gone first.
async fn admit(gone: &Gone) -> Option<Running> {
    let queued = WAITING.with(|waiting| !waiting.borrow().is_empty());

    if ACTIVE.get() == 0 && !queued {
        ACTIVE.set(1);
        PROGRESS.set(Instant::now());

        return Some(Running);
    }

    let (start, mut started) = oneshot::channel();

    WAITING.with(|waiting| waiting.borrow_mut().push_back(start));

    loop {
        let deadline = PROGRESS.get() + max_hold();

        tokio::select! {
            // A start comes first: start_one counted this request in ACTIVE
            biased;
            _ = &mut started => return Some(Running),
            () = gone.wait() => {
                // The coop budget of tokio can make the oneshot answer Pending
                // while its value waits: a start found here is given back
                if started.try_recv().is_ok() {
                    drop(Running);
                }

                return None;
            }
            _ = tokio::time::sleep_until(deadline.into()) => {
                // A start, an end or a park since may have moved the deadline
                if PROGRESS.get() + max_hold() <= Instant::now() {
                    start_one();
                }
            }
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        ACTIVE.set(ACTIVE.get() - 1);
        PROGRESS.set(Instant::now());
        start_one();
    }
}

/// Start the oldest waiting request whose future is still there.
fn start_one() {
    while let Some(start) = WAITING.with(|waiting| waiting.borrow_mut().pop_front()) {
        if start.send(()).is_ok() {
            ACTIVE.set(ACTIVE.get() + 1);
            PROGRESS.set(Instant::now());

            return;
        }
    }
}

/// The thread is about to park, so every running request waits for I/O: one
/// more starts.
pub(crate) fn on_park() {
    PROGRESS.set(Instant::now());
    start_one();
}

#[allow(clippy::arc_with_non_send_sync)]
fn build(bundle: &Bundle, ops: OperationsHandle) -> Result<Place, TerminationReason> {
    // The code is trusted: a stream may live as long as its client reads it
    let limits = RuntimeLimits {
        max_wall_clock_time_ms: 0,
        ..RuntimeLimits::default()
    };
    let isolate = Arc::new(LockerManagedIsolate::new(limits.clone()));
    let context = {
        let (mut locker, _js_lock) = isolate.lock();

        ExecutionContext::new_with_pooled_isolate(
            &mut locker,
            Arc::clone(&isolate),
            isolate.use_snapshot,
            isolate.platform,
            limits,
            bundle.script(),
            ops,
        )?
    };

    Ok(Place { isolate, context })
}

/// Drop the context under the isolate lock, so the handles it holds are freed now.
fn discard(place: Place) {
    let Place { isolate, context } = place;
    let _locker = isolate.lock();

    drop(context);
}

fn take(bundle: &Bundle, ops: OperationsHandle) -> Result<Place, TerminationReason> {
    while let Some(mut place) = FREE.with(|free| free.borrow_mut().pop()) {
        if place.context.reset().is_ok() {
            return Ok(place);
        }

        discard(place);
    }

    build(bundle, ops)
}

async fn run(bundle: Bundle, ops: OperationsHandle, task: Event) -> Result<(), TerminationReason> {
    let mut place = take(&bundle, ops)?;

    place.context.begin_request(None);

    let result = place.context.exec(task).await;
    let drained = match result {
        Ok(()) => place.context.drain_waituntil().await,
        Err(_) => Err(TerminationReason::Other("the request failed".to_string())),
    };

    // A context that failed may hold half a request: it is not reused
    match drained {
        Ok(()) => FREE.with(|free| free.borrow_mut().push(place)),
        Err(_) => discard(place),
    }

    result
}

pub(crate) async fn fetch(
    bundle: Bundle,
    ops: OperationsHandle,
    request: HttpRequest,
    gone: Gone,
) -> Result<HttpResponse, EmbedError> {
    let (task, response) = Event::fetch(request);
    let permit = PERMITS
        .with(Arc::clone)
        .acquire_owned()
        .await
        .expect("the semaphore stays open");

    if gone.is_gone() {
        return Err(EmbedError::Worker(TerminationReason::Aborted));
    }

    respond(
        async move {
            let Some(running) = admit(&gone).await else {
                return Err(TerminationReason::Aborted);
            };

            if gone.is_gone() {
                return Err(TerminationReason::Aborted);
            }

            let result = run(bundle, ops, task).await;

            drop(running);
            drop(permit);

            result
        },
        response,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::HOLD;
    use crate::{Embed, HttpMethod, HttpRequest, RequestBody};
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};
    use tokio::sync::Notify;

    fn get() -> HttpRequest {
        HttpRequest {
            method: HttpMethod::Get,
            url: "http://localhost/".to_string(),
            headers: HashMap::new(),
            body: RequestBody::None,
        }
    }

    fn at(path: &str) -> HttpRequest {
        HttpRequest {
            url: format!("http://localhost{path}"),
            ..get()
        }
    }

    /// Wait until no job runs or waits on the threads of `embed`.
    async fn idle(embed: &Embed) {
        while embed.pool().load() > 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// A worker that counts each request in Rust when it starts; /hold then
    /// waits for `release`, /spin calls a 1 ms Rust command for 300 ms (the
    /// CPU limit ends it at about 50 ms).
    fn counting(counted: &Arc<AtomicUsize>, release: &Arc<Notify>) -> Arc<Embed> {
        let worker = "export default {
            async fetch(request, env) {
                await env.EMBED.invoke('count');

                const path = new URL(request.url).pathname;

                if (path === '/hold') {
                    await env.EMBED.invoke('hold');
                }

                if (path === '/spin') {
                    const end = Date.now() + 300;

                    while (Date.now() < end) {
                        await env.EMBED.invoke('spin');
                    }
                }

                return new Response('ok');
            },
        };";
        let count = counted.clone();
        let hold = release.clone();

        Arc::new(
            Embed::builder(worker)
                .threads(1)
                .command_sync("count", move |_: ()| {
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, String>(())
                })
                .command("hold", move |_: ()| {
                    let hold = hold.clone();

                    async move {
                        hold.notified().await;
                        Ok::<_, String>(())
                    }
                })
                .command_sync("spin", |_: ()| {
                    let end = Instant::now() + Duration::from_millis(1);

                    while Instant::now() < end {}

                    Ok::<_, String>(())
                })
                .build()
                .unwrap(),
        )
    }

    // A request whose caller is gone while it waits for a permit does not run
    #[tokio::test(flavor = "multi_thread")]
    async fn a_request_gone_while_it_waits_for_a_permit_does_not_start() {
        let counted = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Notify::new());
        let embed = counting(&counted, &release);
        let mut holders = tokio::task::JoinSet::new();

        // 8 requests take the 8 permits of the thread and wait
        for _ in 0..8 {
            let embed = embed.clone();

            holders.spawn(async move { embed.fetch(at("/hold")).await.map(|_| ()) });
        }

        while counted.load(Ordering::SeqCst) < 8 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        // 4 more wait for a permit, and their callers give up
        for _ in 0..4 {
            let late = tokio::time::timeout(Duration::from_millis(50), embed.fetch(get())).await;

            assert!(late.is_err(), "a request got a permit that is not free");
        }

        release.notify_waiters();

        while let Some(answer) = holders.join_next().await {
            answer.unwrap().unwrap();
        }

        idle(&embed).await;

        assert_eq!(counted.load(Ordering::SeqCst), 8);
    }

    // A request whose caller is gone while it waits for its turn does not run
    #[tokio::test(flavor = "multi_thread")]
    async fn a_request_gone_while_it_waits_for_its_turn_does_not_start() {
        let counted = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Notify::new());
        let embed = counting(&counted, &release);

        // Only an end or a park starts a waiting request
        embed
            .pool()
            .on_each_thread(|| HOLD.set(Duration::from_secs(60)))
            .await;

        // Warm two places, so the build of an isolate does not count
        let (a, b) = tokio::join!(embed.fetch(get()), embed.fetch(get()));
        a.unwrap();
        b.unwrap();
        idle(&embed).await;

        // One request keeps the thread busy: it never parks. The CPU limit
        // (50 ms) ends it, since its Rust calls run in the turn of JavaScript.
        let spin = {
            let embed = embed.clone();

            tokio::spawn(async move { embed.fetch(at("/spin")).await.map(|_| ()) })
        };

        while counted.load(Ordering::SeqCst) < 3 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        // 2 more get a permit and wait in admit, and their callers give up
        // together, before the CPU limit ends the spinning request
        let late = || tokio::time::timeout(Duration::from_millis(20), embed.fetch(get()));
        let (first, second) = tokio::join!(late(), late());

        assert!(
            first.is_err() && second.is_err(),
            "a request started while another one ran"
        );

        let _ = spin.await.unwrap();
        idle(&embed).await;

        // 2 to warm, 1 spin
        assert_eq!(counted.load(Ordering::SeqCst), 3);
    }

    async fn burst(embed: &Arc<Embed>) {
        let mut calls = tokio::task::JoinSet::new();

        for _ in 0..8 {
            let embed = embed.clone();

            calls.spawn(async move {
                let response = embed.fetch(get()).await.unwrap();

                response.body.collect().await.unwrap()
            });
        }

        while let Some(answer) = calls.join_next().await {
            answer.unwrap();
        }
    }

    // A waiting request gets its start, and its caller leaves before it runs:
    // the start goes back, and the thread does not count it for ever
    #[tokio::test(flavor = "current_thread")]
    async fn a_start_given_to_a_gone_request_goes_back() {
        use super::{ACTIVE, admit};
        use crate::pool::Caller;
        use std::future::Future;
        use std::task::{Context, Poll, Waker};

        for _ in 0..200 {
            let (_stays, stays) = Caller::new();
            let first = admit(&stays).await.unwrap();
            let (caller, gone) = Caller::new();
            let mut waiting = Box::pin(admit(&gone));
            let mut context = Context::from_waker(Waker::noop());

            assert!(matches!(waiting.as_mut().poll(&mut context), Poll::Pending));

            // The end of the first starts the waiting one, whose caller leaves
            drop(first);
            drop(caller);
            drop(waiting.await);

            assert_eq!(ACTIVE.get(), 0);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_setting_that_is_not_utf8_is_an_error() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let not_utf8 = OsString::from_vec(vec![0x38, 0xff]);

        assert!(super::setting(Err(std::env::VarError::NotUnicode(not_utf8))).is_err());
        assert_eq!(
            super::setting(Err(std::env::VarError::NotPresent)).unwrap(),
            None
        );
        assert_eq!(
            super::setting(Ok("2".to_string())).unwrap().as_deref(),
            Some("2")
        );
    }

    #[test]
    fn a_changed_count_of_places_is_an_error() {
        assert!(super::same(8, 8).is_ok());
        assert!(super::same(8, 2).is_err());
    }

    #[test]
    fn a_bad_count_of_places_is_an_error() {
        assert_eq!(super::places(None).unwrap(), super::PLACES);
        assert_eq!(super::places(Some("2")).unwrap(), 2);
        assert!(super::places(Some("0")).is_err());
        assert!(super::places(Some("-1")).is_err());
        assert!(super::places(Some("eight")).is_err());
        assert!(super::places(Some(&usize::MAX.to_string())).is_err());
    }

    // A request waits while another one runs on its thread, and starts when the
    // thread is about to park. MAX_HOLD is off here, so only the park path can
    // start them: this fails if tokio stops checking for work after
    // on_thread_park.
    #[tokio::test(flavor = "multi_thread")]
    async fn requests_that_wait_for_io_overlap_on_one_thread() {
        let worker = "export default {
            async fetch(request, env) {
                await env.EMBED.invoke('wait');

                return new Response('ok');
            },
        };";
        let embed = Arc::new(
            Embed::builder(worker)
                .threads(1)
                .command("wait", |_: ()| async {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    Ok::<_, String>(())
                })
                .build()
                .unwrap(),
        );

        embed
            .pool()
            .on_each_thread(|| HOLD.set(Duration::from_secs(60)))
            .await;

        // Warm the places, so the build of an isolate does not count
        burst(&embed).await;

        let start = Instant::now();

        burst(&embed).await;

        // One after the other, 8 requests take 1.6 s
        let elapsed = start.elapsed();

        eprintln!("8 requests of 200 ms on one thread: {elapsed:?}");
        assert!(elapsed < Duration::from_millis(800), "{elapsed:?}");
    }
}
