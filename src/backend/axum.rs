//! Axum.

use super::hyper;
use crate::Embed;
use axum::Router;
use axum::extract::{Request, State};
use axum::http::Response;
use openworkers_core::HyperBody;
use std::sync::Arc;

/// A router that sends every request to the worker. Mount it as is, or `nest`
/// it behind your own routes.
pub fn router(embed: Embed) -> Router {
    Router::new().fallback(handle).with_state(Arc::new(embed))
}

async fn handle(State(embed): State<Arc<Embed>>, request: Request) -> Response<HyperBody> {
    hyper::handle(&embed, request).await
}
