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

#[cfg(feature = "jsc")]
mod jsc_warm;
#[cfg(feature = "nova")]
mod per_request;
#[cfg(feature = "v8")]
mod warm;

#[cfg(feature = "jsc")]
pub(crate) use jsc_warm::{fetch, init, on_park};
#[cfg(feature = "nova")]
pub(crate) use per_request::{fetch, init, on_park};
#[cfg(feature = "v8")]
pub(crate) use warm::{fetch, init, on_park};

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

            crate::pool::spawn_here(relay_chunks(execution, chunks, relay));

            ResponseBody::Stream(body)
        }
        body => {
            crate::pool::spawn_here(async move {
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
    let mut relay = Relay {
        sender: relay,
        whole: false,
    };
    let mut ended = false;

    loop {
        tokio::select! {
            result = &mut execution, if !ended => {
                ended = true;

                if let Err(reason) = result {
                    relay.whole = true;
                    let _ = relay.sender.send(Err(format!("the worker was stopped: {reason:?}"))).await;

                    return;
                }
            }
            chunk = chunks.recv() => match chunk {
                Some(chunk) => {
                    if relay.sender.send(chunk).await.is_err() {
                        // The client is gone. The worker finishes, as in the runner:
                        // it may still have work to do after its body
                        relay.whole = true;
                        drop(chunks);

                        if !ended {
                            let _ = execution.await;
                        }

                        return;
                    }
                }
                None => {
                    relay.whole = true;

                    if !ended {
                        let _ = execution.await;
                    }

                    return;
                }
            },
        }
    }
}

/// The sender of a relayed body. Dropped before the body is whole, as when
/// the relay panics, it ends the body with an error: a plain end would tell
/// the client that the body is whole.
///
/// Known limit: when the channel is full (16 pieces), the error goes from a
/// new task, which does not run if the runtime is dropping; the client then
/// sees a plain end.
struct Relay {
    sender: mpsc::Sender<Result<Bytes, String>>,
    whole: bool,
}

impl Drop for Relay {
    fn drop(&mut self) {
        if self.whole {
            return;
        }

        let error = Err("the worker stopped while it streamed its body".to_string());

        // A task would not run if the runtime is dropping: try at once first
        let Err(mpsc::error::TrySendError::Full(error)) = self.sender.try_send(error) else {
            return;
        };

        let sender = self.sender.clone();

        tokio::spawn(async move {
            let _ = sender.send(error).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::relay_chunks;
    use bytes::Bytes;
    use openworkers_core::TerminationReason;
    use tokio::sync::{mpsc, oneshot};

    // A relay that panics after a first piece ends the body with an error
    #[tokio::test]
    async fn a_relay_that_panics_ends_the_body_with_an_error() {
        let (chunks_tx, chunks) = mpsc::channel(4);
        let (relay, mut body) = mpsc::channel(4);
        let (go, panic_now) = oneshot::channel::<()>();
        let execution = Box::pin(async move {
            let _ = panic_now.await;

            panic!("the engine panics");

            #[allow(unreachable_code)]
            Ok::<(), TerminationReason>(())
        });

        tokio::spawn(relay_chunks(execution, chunks, relay));
        chunks_tx.send(Ok(Bytes::from("first"))).await.unwrap();

        assert_eq!(body.recv().await.unwrap().unwrap(), "first");

        let _ = go.send(());

        assert!(matches!(body.recv().await, Some(Err(_))));

        drop(chunks_tx);
    }
}
