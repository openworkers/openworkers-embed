//! nova and jsc: a worker per request, as the runner does for every engine
//! without a pool of its own.

use super::respond;
use crate::EmbedError;
use crate::bundle::Bundle;
use crate::pool::Gone;
use openworkers_core::{Event, HttpRequest, HttpResponse, OperationsHandle, TerminationReason};

#[cfg(feature = "jsc")]
use openworkers_runtime_jsc::Worker;
#[cfg(feature = "nova")]
use openworkers_runtime_nova::Worker;

pub(crate) fn init() -> Result<(), EmbedError> {
    Ok(())
}

pub(crate) fn on_park() {}

pub(crate) async fn fetch(
    bundle: Bundle,
    ops: OperationsHandle,
    request: HttpRequest,
    gone: Gone,
) -> Result<HttpResponse, EmbedError> {
    if gone.is_gone() {
        return Err(EmbedError::Worker(TerminationReason::Aborted));
    }

    let mut worker = Worker::new_with_ops(bundle.script(), None, ops)
        .await
        .map_err(EmbedError::Worker)?;
    let (task, response) = Event::fetch(request);

    respond(async move { worker.exec(task).await }, response).await
}
