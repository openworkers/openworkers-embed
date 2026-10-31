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
use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;

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

thread_local! {
    static FREE: RefCell<Vec<Place>> = const { RefCell::new(Vec::new()) };
    static PERMITS: Arc<Semaphore> = Arc::new(Semaphore::new(
        *SETTING.get().expect("init runs before the worker threads"),
    ));
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

#[cfg(test)]
mod tests {
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
}
