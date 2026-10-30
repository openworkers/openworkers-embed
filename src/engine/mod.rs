//! The JavaScript engine of this build: one of v8, nova or jsc, chosen by a
//! feature the way the runner chooses. An engine answers two questions: how to
//! run one request on a worker thread, and which binding types it serves.

use crate::EmbedError;
use bytes::Bytes;
use openworkers_core::{BindingType, HttpResponse, ResponseBody, TerminationReason};
use std::future::Future;
use std::pin::Pin;
use tokio::sync::{mpsc, oneshot};

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
mod warm;

#[cfg(any(feature = "nova", feature = "jsc"))]
pub(crate) use per_request::{fetch, init};
#[cfg(feature = "v8")]
pub(crate) use warm::{fetch, init};

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

/// The response of an event, as soon as the worker has one. `execution` drives the
/// event; once the response is there it moves to a task of its own, so a body that
/// streams keeps being fed while the caller reads it.
async fn respond<F>(
    execution: F,
    response: oneshot::Receiver<HttpResponse>,
) -> Result<HttpResponse, EmbedError>
where
    F: Future<Output = Result<(), TerminationReason>> + 'static,
{
    let mut execution = Box::pin(execution);
    let mut response = response;

    tokio::select! {
        result = &mut execution => {
            result.map_err(EmbedError::Worker)?;
            response.await.map_err(|_| EmbedError::NoResponse)
        }
        answer = &mut response => match answer {
            Ok(mut response) => {
                response.body = follow(execution, response.body);

                Ok(response)
            }
            // No response: the execution says why
            Err(_) => {
                execution.await.map_err(EmbedError::Worker)?;

                Err(EmbedError::NoResponse)
            }
        },
    }
}

type Chunks = mpsc::Receiver<Result<Bytes, String>>;

/// Keeps the worker running while the caller reads its body. A body that streams
/// is relayed, so that it ends with an error when the runtime stops the worker:
/// left alone, it would stay open for ever, and the client would wait.
fn follow<F>(execution: Pin<Box<F>>, body: ResponseBody) -> ResponseBody
where
    F: Future<Output = Result<(), TerminationReason>> + 'static,
{
    match body {
        ResponseBody::Stream(chunks) => {
            let (relay, body) = mpsc::channel(16);

            tokio::task::spawn_local(relay_chunks(execution, chunks, relay));

            ResponseBody::Stream(body)
        }
        body => {
            tokio::task::spawn_local(async move {
                let _ = execution.await;
            });

            body
        }
    }
}

async fn relay_chunks<F>(
    mut execution: Pin<Box<F>>,
    mut chunks: Chunks,
    relay: mpsc::Sender<Result<Bytes, String>>,
) where
    F: Future<Output = Result<(), TerminationReason>>,
{
    let mut ended = false;

    loop {
        tokio::select! {
            result = &mut execution, if !ended => {
                ended = true;

                if let Err(reason) = result {
                    let _ = relay.send(Err(format!("the worker was stopped: {reason:?}"))).await;

                    return;
                }
            }
            chunk = chunks.recv() => match chunk {
                Some(chunk) => {
                    if relay.send(chunk).await.is_err() {
                        // The client is gone. The worker finishes, as in the runner:
                        // it may still have work to do after its body
                        drop(chunks);

                        if !ended {
                            let _ = execution.await;
                        }

                        return;
                    }
                }
                None => {
                    if !ended {
                        let _ = execution.await;
                    }

                    return;
                }
            },
        }
    }
}
