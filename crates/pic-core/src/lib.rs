//! Shared processing core. The CLI only parses arguments and renders results.

pub mod capabilities;
pub mod codec;
pub mod composite;
pub mod document;
pub mod error;
pub mod limits;
pub mod operation;
pub mod pipeline;
pub mod project;
pub mod result;
pub mod text;

pub use error::{ErrorCode, PicError, Result};
