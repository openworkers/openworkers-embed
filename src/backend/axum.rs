//! Axum.

use super::hyper;
use crate::{Embed, EmbedError};
use axum::Router;
use axum::extract::{Request, State};
use axum::http::Response;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use openworkers_core::HyperBody;
use std::sync::Arc;

/// A router that sends every request to the worker. Mount it as is, or `nest`
/// it behind your own routes.
pub fn router(embed: Embed) -> Router {
    Router::new().fallback(handle).with_state(Arc::new(embed))
}

/// Serve [`router`] on the worker threads of `embed`, the way Node and Bun
/// serve: each worker thread accepts connections, reads their requests and
/// runs the worker for them. A request never crosses a thread. HTTP/1 only.
///
/// With [`axum::serve`] and [`router`], the requests arrive on the threads of
/// the tokio runtime and go to a worker thread and back: two threads to wake
/// for each request.
pub async fn serve(listener: std::net::TcpListener, embed: Embed) -> Result<(), EmbedError> {
    listener.set_nonblocking(true).map_err(EmbedError::Serve)?;

    let embed = Arc::new(embed);
    let app = Router::new()
        .fallback(handle)
        .with_state(Arc::clone(&embed));
    let listener = Arc::new(listener);
    let (failed, mut failure) = tokio::sync::mpsc::unbounded_channel();

    embed.on_each_thread(move |_| {
        let listener = listener.try_clone();
        let app = app.clone();
        let failed = failed.clone();

        async move {
            let listener = match listener.and_then(tokio::net::TcpListener::from_std) {
                Ok(listener) => listener,
                Err(error) => {
                    let _ = failed.send(error);

                    return;
                }
            };

            accept(listener, app).await;
        }
    })?;

    match failure.recv().await {
        Some(error) => Err(EmbedError::Serve(error)),
        None => Ok(()),
    }
}

async fn accept(listener: tokio::net::TcpListener, app: Router) {
    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            // Too many open files, for instance: the next accept may work
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;

                continue;
            }
        };

        let _ = stream.set_nodelay(true);
        let service = TowerToHyperService::new(app.clone());

        tokio::task::spawn_local(async move {
            let _ = ::hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await;
        });
    }
}

async fn handle(State(embed): State<Arc<Embed>>, request: Request) -> Response<HyperBody> {
    hyper::handle(&embed, request).await
}
