//! v8: each worker thread keeps warm places, an isolate with the one context
//! that runs the bundle. A request takes a place, resets its context and
//! gives it back, so the bundle runs once per place.

use super::respond;
use crate::EmbedError;
use crate::bundle::Bundle;
use openworkers_core::{
    Event, HttpRequest, HttpResponse, OperationsHandle, RuntimeLimits, TerminationReason,
};
use openworkers_runtime_v8::{ExecutionContext, LockerManagedIsolate};
use std::cell::RefCell;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// Requests in flight on one thread. A stream keeps its place until the
/// client has read it, so this is also the number of streams a thread serves.
const PLACES: usize = 8;

struct Place {
    isolate: Arc<LockerManagedIsolate>,
    context: ExecutionContext,
}

thread_local! {
    static FREE: RefCell<Vec<Place>> = const { RefCell::new(Vec::new()) };
    static PERMITS: Arc<Semaphore> = Arc::new(Semaphore::new(
        std::env::var("INV_PLACES").ok().and_then(|n| n.parse().ok()).unwrap_or(PLACES),
    ));
}

pub(crate) fn init() {}

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
) -> Result<HttpResponse, EmbedError> {
    let (task, response) = Event::fetch(request);
    let permit = PERMITS
        .with(Arc::clone)
        .acquire_owned()
        .await
        .expect("the semaphore stays open");

    respond(
        async move {
            let result = run(bundle, ops, task).await;

            drop(permit);

            result
        },
        response,
    )
    .await
}
