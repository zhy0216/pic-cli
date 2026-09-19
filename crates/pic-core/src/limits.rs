use std::{fs, io::Read, path::Path};

use serde::Serialize;

use crate::{ErrorCode, PicError, Result};

/// Admission limits, not a process RSS limit; codec scratch allocations are best effort.
#[derive(Debug, Clone, Serialize)]
pub struct ResourceLimits {
    pub max_dimension: u32,
    pub max_pixels: u64,
    pub max_input_bytes: u64,
    pub max_buffer_bytes: u64,
    pub max_pipeline_bytes: u64,
    pub max_operations: usize,
    /// Total manifest plus committed operation bytes, excluding encoded assets.
    pub max_project_bytes: u64,
    /// Includes inactive, still traceable history.
    pub max_history_operations: usize,
    /// Combined managed checkpoint and preview files; never includes required assets.
    pub max_cache_disk_bytes: u64,
    /// Retained derived rasters and cache serialization/read scratch admission.
    pub max_cache_memory_bytes: u64,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_dimension: 32_768,
            max_pixels: 40_000_000,
            max_input_bytes: 128 * 1024 * 1024,
            max_buffer_bytes: 1024 * 1024 * 1024,
            max_pipeline_bytes: 1024 * 1024,
            max_operations: 10_000,
            max_project_bytes: 64 * 1024 * 1024,
            max_history_operations: 100_000,
            max_cache_disk_bytes: 512 * 1024 * 1024,
            max_cache_memory_bytes: 256 * 1024 * 1024,
        }
    }
}

impl ResourceLimits {
    pub fn check_dimensions(&self, width: u32, height: u32) -> Result<usize> {
        if width == 0 || height == 0 {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "image dimensions must be positive",
            ));
        }
        let pixels = u64::from(width) * u64::from(height);
        // 16 bytes of RGBA32F plus allowance for decoded and encoded 8-bit buffers.
        let working_bytes = pixels.checked_mul(32);
        if width > self.max_dimension
            || height > self.max_dimension
            || pixels > self.max_pixels
            || working_bytes.is_none_or(|bytes| bytes > self.max_buffer_bytes)
        {
            return Err(PicError::new(
                ErrorCode::ResourceLimit,
                format!("{width}x{height} exceeds image or buffer limits"),
            ));
        }
        usize::try_from(pixels)
            .map_err(|_| PicError::new(ErrorCode::ResourceLimit, "image exceeds address space"))
    }

    pub(crate) fn decoder_limits(&self) -> image::Limits {
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(self.max_dimension);
        limits.max_image_height = Some(self.max_dimension);
        limits.max_alloc = Some(self.max_buffer_bytes);
        limits
    }
}

pub(crate) fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>> {
    if path.to_str().is_none() {
        return Err(PicError::new(
            ErrorCode::InvalidArgument,
            "file path must be valid UTF-8",
        ));
    }
    let metadata = fs::metadata(path).map_err(|e| PicError::io("inspect", path, e))?;
    if !metadata.is_file() {
        return Err(PicError::new(
            ErrorCode::InvalidArgument,
            format!("'{}' is not a regular file", path.display()),
        ));
    }
    if metadata.len() > limit {
        return Err(PicError::new(
            ErrorCode::ResourceLimit,
            format!("'{}' exceeds {limit} bytes", path.display()),
        ));
    }
    let file = fs::File::open(path).map_err(|e| PicError::io("open", path, e))?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| PicError::io("read", path, e))?;
    if bytes.len() as u64 > limit {
        return Err(PicError::new(
            ErrorCode::ResourceLimit,
            "file grew beyond byte limit while reading",
        ));
    }
    Ok(bytes)
}
