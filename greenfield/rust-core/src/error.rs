use std::time::Duration;

use napi::{Error as NapiError, Status};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("operation timed out after {0:?}")]
    Timeout(Duration),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type CoreResult<T> = std::result::Result<T, CoreError>;

impl From<CoreError> for NapiError {
    fn from(value: CoreError) -> Self {
        match value {
            CoreError::Timeout(duration) => NapiError::new(
                Status::Cancelled,
                format!("operation timed out after {:?}", duration),
            ),
            CoreError::InvalidArgument(msg) => NapiError::new(Status::InvalidArg, msg),
            CoreError::Other(err) => NapiError::new(Status::GenericFailure, err.to_string()),
        }
    }
}

pub fn invalid_argument(message: impl Into<String>) -> CoreError {
    CoreError::InvalidArgument(message.into())
}

pub fn timeout(duration: Duration) -> CoreError {
    CoreError::Timeout(duration)
}
