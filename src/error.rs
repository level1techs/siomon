//! Error types (`SiomonError`, `NvmlError`) and the crate-level `Result` alias.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SiomonError {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    ParseInt(#[from] std::num::ParseIntError),

    #[error(transparent)]
    ParseFloat(#[from] std::num::ParseFloatError),
}

#[derive(Debug, Error)]
pub enum NvmlError {
    #[error("NVML returned error code {0}")]
    ApiError(u32),
}

pub type Result<T> = std::result::Result<T, SiomonError>;
