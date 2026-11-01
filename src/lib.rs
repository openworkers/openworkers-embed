//! Run a worker (a SvelteKit server bundle, for instance) inside a Rust backend.
//!
//! The worker calls Rust functions the host registers with
//! [`EmbedBuilder::command`], the way a Tauri frontend calls its commands:
//! `await env.EMBED.invoke("name", args)`, or `platform.invoke` in SvelteKit.
//! Requests come in through [`Embed::fetch`], or through a [`backend`] for actix,
//! hyper or axum.

mod assets;
mod bundle;
mod commands;
mod embed;
mod engine;
mod error;
#[cfg(feature = "fetch")]
mod fetch;
mod host;
mod pool;
mod response;
mod url;

pub mod backend;
#[cfg(feature = "v8")]
pub mod snapshot;

pub use embed::{Embed, EmbedBuilder};
pub use error::EmbedError;
pub use openworkers_core::{HttpMethod, HttpRequest, HttpResponse, RequestBody, ResponseBody};
