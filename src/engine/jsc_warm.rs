//! jsc: each worker thread keeps warm workers. A request takes one, resets
//! it, runs its event and gives it back, so the bundle is evaluated once per
//! worker, not once per request. The CPU limit of runtime-jsc cuts the top
//! level, each event and the reset; a worker that was cut or failed is not
//! used again.

use super::respond;
use crate::EmbedError;
use crate::bundle::Bundle;
use crate::pool::Gone;
use openworkers_core::{Event, HttpRequest, HttpResponse, OperationsHandle, TerminationReason};
use openworkers_runtime_jsc::JscOptions;
use openworkers_runtime_jsc::Worker;
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::OnceLock;
use tokio::sync::Semaphore;

/// Requests in flight on one thread, and so warm workers at most.
const PLACES: usize = 8;

thread_local! {
    static FREE: RefCell<Vec<Worker>> = const { RefCell::new(Vec::new()) };
    static PERMITS: Arc<Semaphore> = Arc::new(Semaphore::new(PLACES));
}

/// The outcome of the JSC options, set once per process.
static OPTIONS: OnceLock<Result<(), String>> = OnceLock::new();

/// Turns on the polling traps of JSC, before the worker threads start: with
/// the JIT, the CPU limit cuts a loop that calls native code only with them.
/// Fails closed when JSC already started without them.
pub(crate) fn init() -> Result<(), EmbedError> {
    let outcome = OPTIONS.get_or_init(|| {
        let options = JscOptions {
            polling_traps: true,
        };

        // SAFETY: the README asks to build the Embed, or to call init_jsc,
        // before other threads start; a call after the start writes nothing
        unsafe { openworkers_runtime_jsc::init_jsc(options) }.map_err(|_| {
            "JavaScriptCore started without polling traps: build the Embed, or call \
             init_jsc with polling_traps, before the first JavaScriptCore context"
                .to_string()
        })
    });

    outcome.clone().map_err(EmbedError::Setting)
}

pub(crate) fn on_park() {}

pub(crate) async fn fetch(
    bundle: Bundle,
    ops: OperationsHandle,
    request: HttpRequest,
    gone: Gone,
) -> Result<HttpResponse, EmbedError> {
    let permit = PERMITS
        .with(Arc::clone)
        .acquire_owned()
        .await
        .expect("the semaphore stays open");

    if gone.is_gone() {
        return Err(EmbedError::Worker(TerminationReason::Aborted));
    }

    let worker = loop {
        match FREE.with(|free| free.borrow_mut().pop()) {
            Some(mut worker) => {
                if worker.reset().is_ok() {
                    break worker;
                }
            }
            None => {
                break Worker::new_with_ops(bundle.script(), None, ops)
                    .await
                    .map_err(EmbedError::Worker)?;
            }
        }
    };
    let (task, response) = Event::fetch(request);

    respond(
        async move {
            let mut worker = worker;
            let result = worker.exec(task).await;

            // A worker whose event failed may hold half an event: not reused
            if result.is_ok() {
                FREE.with(|free| free.borrow_mut().push(worker));
            }

            drop(permit);

            result
        },
        response,
    )
    .await
}
