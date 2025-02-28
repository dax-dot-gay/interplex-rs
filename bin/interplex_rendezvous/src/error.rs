use thiserror::Error;

#[derive(Error, Clone, Debug)]
pub(crate) enum ServerError {
    #[error("Invalid expose argument: {0}")]
    InvalidExpose(String),
}
