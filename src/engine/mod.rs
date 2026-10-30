//! The JavaScript engine of this build: one of v8, nova or jsc, chosen by a
//! feature the way the runner chooses. An engine answers two questions: how to
//! run one request on a worker thread, and which binding types it serves.

use crate::EmbedError;
use openworkers_core::{BindingType, HttpResponse, ResponseBody};
use tokio::sync::oneshot;

#[cfg(not(any(feature = "v8", feature = "nova", feature = "jsc")))]
compile_error!("no JavaScript engine selected: build with --features v8|nova|jsc");

/// Engines this build carries. None selected is left to the `compile_error!`
/// above, which fires before name resolution and so reports alone.
const ENGINES: usize = cfg!(feature = "v8") as usize
    + cfg!(feature = "nova") as usize
    + cfg!(feature = "jsc") as usize;

const _: () = assert!(
    ENGINES <= 1,
    "JavaScript engines are mutually exclusive: select one of v8|nova|jsc"
);

#[cfg(any(feature = "nova", feature = "jsc"))]
mod per_request;
#[cfg(feature = "v8")]
mod pinned;

#[cfg(any(feature = "nova", feature = "jsc"))]
pub(crate) use per_request::{fetch, init};
#[cfg(feature = "v8")]
pub(crate) use pinned::{fetch, init};

/// Binding types the engine serves: v8 builds an `env` object per binding,
/// while nova and jsc list what they serve themselves.
#[cfg(feature = "v8")]
const SUPPORTED: &[BindingType] = &[
    BindingType::Assets,
    BindingType::Storage,
    BindingType::Kv,
    BindingType::Database,
    BindingType::D1,
    BindingType::Worker,
    BindingType::DurableObject,
    BindingType::Invoke,
];
#[cfg(feature = "nova")]
const SUPPORTED: &[BindingType] = openworkers_runtime_nova::SUPPORTED_BINDINGS;
#[cfg(feature = "jsc")]
const SUPPORTED: &[BindingType] = openworkers_runtime_jsc::SUPPORTED_BINDINGS;

pub(crate) fn supports(binding_type: BindingType) -> bool {
    SUPPORTED.contains(&binding_type)
}

/// The response of an event, with its body read to the end.
async fn buffered(response: oneshot::Receiver<HttpResponse>) -> Result<HttpResponse, EmbedError> {
    let response = response.await.map_err(|_| EmbedError::NoResponse)?;
    let body = response
        .body
        .collect()
        .await
        .map_err(EmbedError::Body)?
        .unwrap_or_default();

    Ok(HttpResponse {
        status: response.status,
        headers: response.headers,
        body: ResponseBody::Bytes(body),
    })
}
