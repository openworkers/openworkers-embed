//! Run a worker (a SvelteKit server bundle, for instance) inside a Rust backend.
//!
//! The worker calls Rust functions the host registers with
//! [`EmbedBuilder::command`], the way a Tauri frontend calls its commands:
//! `await embed.invoke("name", args)`. Requests come in through
//! [`Embed::fetch`], or through a [`backend`] for actix, hyper or axum.

mod assets;
mod bundle;
mod commands;
mod embed;
mod engine;
mod error;
mod host;
mod pool;
mod response;
mod url;

pub mod backend;

pub use embed::{Embed, EmbedBuilder};
pub use error::EmbedError;
pub use openworkers_core::{HttpMethod, HttpRequest, HttpResponse, RequestBody, ResponseBody};
