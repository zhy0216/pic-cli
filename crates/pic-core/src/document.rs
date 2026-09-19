//! Shared pixel state and identities. Persistent assets and ops live in `project`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{ErrorCode, PicError, Result, limits::ResourceLimits};

pub const DOCUMENT_SCHEMA_VERSION: u32 = 1;
pub const PIXEL_SEMANTICS: &str = "linear_srgb_rgba32f_straight_v1";

/// Opaque identity, never a display name or an index. Stateless images use `canvas`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TargetId(pub String);

impl TargetId {
    pub fn canvas() -> Self {
        Self("canvas".into())
    }
}

/// An immutable state identifier, independent of the document schema version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RevisionId(pub String);

/// Pixels are linear-light sRGB, straight (unassociated) alpha, in row-major RGBA order.
/// RGB may be finite HDR/negative values; alpha is finite and in [0, 1].
/// Clones share immutable samples. Checkpoints preserve every f32 bit.
#[derive(Debug, Clone)]
pub struct Raster {
    width: u32,
    height: u32,
    pixels: Arc<Vec<[f32; 4]>>,
}

impl Raster {
    pub fn from_linear_rgba(
        width: u32,
        height: u32,
        pixels: Vec<[f32; 4]>,
        limits: &ResourceLimits,
    ) -> Result<Self> {
        let count = limits.check_dimensions(width, height)?;
        if pixels.len() != count {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "pixel count does not match dimensions",
            ));
        }
        if pixels
            .iter()
            .any(|p| p.iter().any(|v| !v.is_finite()) || !(0.0..=1.0).contains(&p[3]))
        {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "pixels must be finite and alpha must be in [0, 1]",
            ));
        }
        Ok(Self {
            width,
            height,
            pixels: Arc::new(pixels),
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn pixels(&self) -> &[[f32; 4]] {
        self.pixels.as_slice()
    }
    pub fn has_transparency(&self) -> bool {
        self.pixels.iter().any(|pixel| pixel[3] < 1.0)
    }
}
