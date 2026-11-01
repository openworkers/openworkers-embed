//! Axum.

use super::hyper;
use crate::Embed;
use axum::Router;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::Response;
use openworkers_core::HyperBody;
use std::net::SocketAddr;
use std::sync::Arc;

/// A router that sends every request to the worker. Mount it as is, or `nest`
/// it behind your own routes.
///
/// Serve it with `into_make_service_with_connect_info::<SocketAddr>()` for the
/// worker to get the address of the client in `x-real-ip`; without it, the
/// worker gets no `x-real-ip`.
pub fn router(embed: Embed) -> Router {
    Router::new().fallback(handle).with_state(Arc::new(embed))
}

async fn handle(State(embed): State<Arc<Embed>>, request: Request) -> Response<HyperBody> {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0);

    hyper::forward(&embed, request, peer).await
}
