use openworkers_core::TerminationReason;

#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("the worker failed: {0:?}")]
    Worker(TerminationReason),
    #[error("the worker sent no response")]
    NoResponse,
    #[error("the worker thread stopped")]
    ThreadGone,
    #[error("the worker code does not parse: {0}")]
    Code(String),
}
