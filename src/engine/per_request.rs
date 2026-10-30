//! nova and jsc: a worker per request, as the runner does for every engine
//! without a pool of its own.

use super::respond;
use crate::EmbedError;
use crate::bundle::Bundle;
use openworkers_core::{Event, HttpRequest, HttpResponse, OperationsHandle};

#[cfg(feature = "jsc")]
use openworkers_runtime_jsc::Worker;
#[cfg(feature = "nova")]
use openworkers_runtime_nova::Worker;

pub(crate) fn init() {}

pub(crate) async fn fetch(
    bundle: Bundle,
    ops: OperationsHandle,
    request: HttpRequest,
) -> Result<HttpResponse, EmbedError> {
    let mut worker = Worker::new_with_ops(bundle.script(), None, ops)
        .await
        .map_err(EmbedError::Worker)?;
    let (task, response) = Event::fetch(request);

    respond(async move { worker.exec(task).await }, response).await
}
