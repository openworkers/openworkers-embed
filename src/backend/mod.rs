//! Web frameworks `openworkers-core` converts requests and responses for.
//! Each one is behind a feature of the same name.

#[cfg(feature = "actix")]
pub mod actix;
#[cfg(feature = "axum")]
pub mod axum;
#[cfg(feature = "hyper")]
pub mod hyper;
