use std::{fmt, io, path::Path};

use serde::Serialize;

pub type Result<T> = std::result::Result<T, PicError>;

/// Machine-readable codes are part of result schema version 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidArgument,
    InvalidJson,
    UnsupportedVersion,
    UnknownOperation,
    InvalidTarget,
    InvalidMask,
    UnsupportedTemplate,
    FileNotFound,
    IoError,
    UnsupportedFormat,
    UnsupportedColor,
    UnsupportedMetadata,
    DecodeFailed,
    EncodeFailed,
    OutputExists,
    AlphaNotSupported,
    ResourceLimit,
    InvalidProject,
    AssetMissing,
    IntegrityMismatch,
    UnsafePath,
    RevisionNotFound,
    RevisionConflict,
    HistoryBoundary,
}

#[derive(Debug, Clone, Serialize)]
pub struct PicError {
    pub code: ErrorCode,
    pub message: String,
}

impl PicError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn io(action: &str, path: &Path, error: io::Error) -> Self {
        let code = match error.kind() {
            io::ErrorKind::NotFound => ErrorCode::FileNotFound,
            io::ErrorKind::AlreadyExists => ErrorCode::OutputExists,
            _ => ErrorCode::IoError,
        };
        Self::new(code, format!("{action} '{}': {error}", path.display()))
    }

    pub(crate) fn image(error: image::ImageError, fallback: ErrorCode) -> Self {
        let code = match error {
            image::ImageError::Limits(_) => ErrorCode::ResourceLimit,
            image::ImageError::Unsupported(_) => ErrorCode::UnsupportedFormat,
            _ => fallback,
        };
        Self::new(code, error.to_string())
    }
}

impl fmt::Display for PicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PicError {}
