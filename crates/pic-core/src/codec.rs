//! The only boundary that converts between encoded bytes, 8-bit sRGB and working pixels.

mod exif;
mod input_contract;

use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    str::FromStr,
};

use image::{ColorType, ImageDecoder, ImageEncoder, ImageFormat};
use serde::Serialize;

use crate::{
    ErrorCode, PicError, Result,
    document::Raster,
    limits::{ResourceLimits, read_limited},
    operation::geometry::RgbaColor,
    result::{Diagnostics, Warning, timed},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Png,
    Jpeg,
}

impl FromStr for Format {
    type Err = PicError;
    fn from_str(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "png" => Ok(Self::Png),
            "jpg" | "jpeg" => Ok(Self::Jpeg),
            _ => Err(PicError::new(
                ErrorCode::UnsupportedFormat,
                "supported output formats: png, jpeg (jpg)",
            )),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EncodeOptions {
    pub format: Option<Format>,
    /// None selects 90 for JPEG. Specifying this for PNG is an error.
    pub jpeg_quality: Option<u8>,
    /// PNG deflate level 0..9 (default 6); invalid for JPEG.
    pub png_compression: Option<u8>,
    /// Explicit opaque sRGB background for linear-light JPEG flattening.
    pub jpeg_background: Option<RgbaColor>,
}

#[derive(Debug)]
pub struct ResolvedEncoding {
    pub format: Format,
    pub jpeg_quality: Option<u8>,
    pub png_compression: Option<u8>,
    pub jpeg_background: Option<RgbaColor>,
}

impl EncodeOptions {
    pub fn resolve(&self, output: &Path) -> Result<ResolvedEncoding> {
        let format = match self.format {
            Some(format) => format,
            None => output
                .extension()
                .and_then(|ext| ext.to_str())
                .ok_or_else(|| {
                    PicError::new(
                        ErrorCode::UnsupportedFormat,
                        "output requires .png/.jpg/.jpeg or an explicit --format",
                    )
                })?
                .parse()?,
        };
        if self
            .jpeg_quality
            .is_some_and(|quality| !(1..=100).contains(&quality))
        {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "JPEG quality must be in [1, 100]",
            ));
        }
        if format == Format::Png && (self.jpeg_quality.is_some() || self.jpeg_background.is_some())
        {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "JPEG quality/background are not PNG parameters",
            ));
        }
        if self.png_compression.is_some_and(|level| level > 9) {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "PNG compression must be in [0, 9]",
            ));
        }
        if format == Format::Jpeg && self.png_compression.is_some() {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "PNG compression is not a JPEG parameter",
            ));
        }
        if self.jpeg_background.is_some_and(|color| color.0[3] != 255) {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "JPEG background must be opaque",
            ));
        }
        Ok(ResolvedEncoding {
            format,
            jpeg_quality: (format == Format::Jpeg).then_some(self.jpeg_quality.unwrap_or(90)),
            png_compression: (format == Format::Png).then_some(self.png_compression.unwrap_or(6)),
            jpeg_background: self.jpeg_background,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct ImageInfo {
    pub path: PathBuf,
    pub format: Format,
    pub width: u32,
    pub height: u32,
    pub stored_width: u32,
    pub stored_height: u32,
    pub exif_orientation: Option<u8>,
    pub bit_depth: u8,
    pub color_type: &'static str,
    pub color_space: &'static str,
    pub color_source: &'static str,
    pub has_alpha: bool,
    pub has_transparency: bool,
    pub orientation: &'static str,
}

pub struct LoadedImage {
    pub raster: Raster,
    pub info: ImageInfo,
}

pub fn load(
    path: &Path,
    limits: &ResourceLimits,
    diagnostics: &mut Diagnostics,
) -> Result<LoadedImage> {
    let (path, bytes) = timed(&mut diagnostics.timings.read_ms, || {
        let path = fs::canonicalize(path).map_err(|e| PicError::io("resolve input", path, e))?;
        let bytes = read_limited(&path, limits.max_input_bytes)?;
        Ok((path, bytes))
    })?;
    load_bytes(&bytes, path, limits, diagnostics)
}

/// Decode the exact bytes already read and verified by the project asset store.
/// The path is diagnostic provenance only; it is never reopened here.
pub fn load_bytes(
    bytes: &[u8],
    path: PathBuf,
    limits: &ResourceLimits,
    diagnostics: &mut Diagnostics,
) -> Result<LoadedImage> {
    if bytes.len() as u64 > limits.max_input_bytes {
        return Err(PicError::new(
            ErrorCode::ResourceLimit,
            "image exceeds byte limit",
        ));
    }
    let loaded = timed(&mut diagnostics.timings.decode_ms, || {
        decode(bytes, path, limits, false)
    })?;
    if loaded.info.color_source == "assumed_srgb" {
        diagnostics.warnings.push(Warning { code: "assumed_srgb", message: "Untagged input is interpreted as sRGB; no color profile conversion is performed." });
    }
    if loaded.info.exif_orientation.is_some_and(|value| value != 1) {
        diagnostics.warnings.push(Warning { code: "orientation_applied", message: "EXIF orientation was applied before canvas operations; dimensions and coordinates use normalized pixels." });
    }
    Ok(loaded)
}

/// External masks use encoded grayscale coverage, never sRGB-decoded luminance.
pub fn load_mask_bytes(bytes: &[u8], path: PathBuf, limits: &ResourceLimits) -> Result<Raster> {
    if bytes.len() as u64 > limits.max_input_bytes {
        return Err(PicError::new(
            ErrorCode::ResourceLimit,
            "mask exceeds byte limit",
        ));
    }
    Ok(decode(bytes, path, limits, true)?.raster)
}

fn decode(
    bytes: &[u8],
    path: PathBuf,
    limits: &ResourceLimits,
    coverage: bool,
) -> Result<LoadedImage> {
    #[cfg(test)]
    tests::DECODE_CALLS.with(|calls| calls.set(calls.get() + 1));
    let image_format = image::guess_format(bytes).map_err(|_| {
        PicError::new(
            ErrorCode::UnsupportedFormat,
            "input is not a recognized PNG or JPEG",
        )
    })?;
    let (format, metadata) = match image_format {
        ImageFormat::Png => (Format::Png, input_contract::png(bytes)?),
        ImageFormat::Jpeg => (Format::Jpeg, input_contract::jpeg(bytes)?),
        _ => {
            return Err(PicError::new(
                ErrorCode::UnsupportedFormat,
                "only PNG and JPEG inputs are supported",
            ));
        }
    };
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image_format);
    reader.limits(limits.decoder_limits());
    let decoder = reader
        .into_decoder()
        .map_err(|e| PicError::image(e, ErrorCode::DecodeFailed))?;
    let (width, height) = decoder.dimensions();
    let count = limits.check_dimensions(width, height)?;
    let color = decoder.color_type();
    let color_type = match color {
        ColorType::L8 => "gray",
        ColorType::La8 => "gray_alpha",
        ColorType::Rgb8 => "rgb",
        ColorType::Rgba8 => "rgba",
        _ => {
            return Err(PicError::new(
                ErrorCode::UnsupportedColor,
                "only 8-bit grayscale/RGB/RGBA inputs are supported",
            ));
        }
    };
    let mut decoded = image::DynamicImage::from_decoder(decoder)
        .map_err(|e| PicError::image(e, ErrorCode::DecodeFailed))?;
    if let Some(value) = metadata.exif_orientation {
        let orientation = image::metadata::Orientation::from_exif(value).ok_or_else(|| {
            PicError::new(ErrorCode::UnsupportedMetadata, "invalid EXIF orientation")
        })?;
        decoded.apply_orientation(orientation);
    }
    let decoded = decoded.into_rgba8();
    let (stored_width, stored_height) = (width, height);
    let (width, height) = decoded.dimensions();
    limits.check_dimensions(width, height)?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(count)
        .map_err(|_| PicError::new(ErrorCode::ResourceLimit, "cannot allocate working pixels"))?;
    if coverage && decoded.pixels().any(|p| p[0] != p[1] || p[1] != p[2]) {
        return Err(PicError::new(
            ErrorCode::InvalidMask,
            "mask must contain grayscale coverage (R=G=B), optionally multiplied by alpha",
        ));
    }
    pixels.extend(decoded.pixels().map(|p| {
        if coverage {
            [
                0.0,
                0.0,
                0.0,
                (f64::from(p[0]) * f64::from(p[3]) / (255.0 * 255.0)) as f32,
            ]
        } else {
            [
                srgb_to_linear(p[0]),
                srgb_to_linear(p[1]),
                srgb_to_linear(p[2]),
                f32::from(p[3]) / 255.0,
            ]
        }
    }));
    let raster = Raster::from_linear_rgba(width, height, pixels, limits)?;
    let info = ImageInfo {
        path,
        format,
        width,
        height,
        stored_width,
        stored_height,
        exif_orientation: metadata.exif_orientation,
        bit_depth: 8,
        color_type,
        color_space: "srgb",
        color_source: if metadata.declared_srgb {
            "declared_srgb"
        } else {
            "assumed_srgb"
        },
        has_alpha: color.has_alpha(),
        has_transparency: raster.has_transparency(),
        orientation: if metadata.exif_orientation.is_some() {
            "exif_normalized"
        } else {
            "stored_pixels"
        },
    };
    Ok(LoadedImage { raster, info })
}

pub(crate) fn srgb_to_linear(sample: u8) -> f32 {
    let value = f32::from(sample) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let encoded = if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

pub fn encode(
    raster: &Raster,
    options: &ResolvedEncoding,
    limits: &ResourceLimits,
) -> Result<Vec<u8>> {
    #[cfg(test)]
    tests::ENCODE_CALLS.with(|calls| calls.set(calls.get() + 1));
    let count = limits.check_dimensions(raster.width(), raster.height())?;
    // Validate even for library callers constructing ResolvedEncoding themselves.
    let options = EncodeOptions {
        format: Some(options.format),
        jpeg_quality: options.jpeg_quality,
        png_compression: options.png_compression,
        jpeg_background: options.jpeg_background,
    }
    .resolve(Path::new("output"))?;
    if options.format == Format::Jpeg
        && raster.has_transparency()
        && options.jpeg_background.is_none()
    {
        return Err(PicError::new(
            ErrorCode::AlphaNotSupported,
            "JPEG cannot preserve transparency; export PNG or specify an opaque --jpeg-background",
        ));
    }
    let channels = if options.format == Format::Png { 4 } else { 3 };
    let mut samples = Vec::new();
    samples
        .try_reserve_exact(count * channels)
        .map_err(|_| PicError::new(ErrorCode::ResourceLimit, "cannot allocate export samples"))?;
    let background = options.jpeg_background.map(RgbaColor::linear);
    for pixel in raster.pixels() {
        if let Some(background) = background {
            samples.extend((0..3).map(|channel| {
                linear_to_srgb(pixel[channel] * pixel[3] + background[channel] * (1.0 - pixel[3]))
            }));
        } else {
            samples.extend(pixel[..3].iter().map(|value| linear_to_srgb(*value)));
        }
        if channels == 4 {
            samples.push((pixel[3] * 255.0).round() as u8);
        }
    }
    let mut encoded = Vec::new();
    match options.format {
        Format::Png => image::codecs::png::PngEncoder::new_with_quality(
            &mut encoded,
            image::codecs::png::CompressionType::Level(options.png_compression.unwrap_or(6)),
            image::codecs::png::FilterType::Adaptive,
        )
        .write_image(
            &samples,
            raster.width(),
            raster.height(),
            image::ExtendedColorType::Rgba8,
        ),
        Format::Jpeg => image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut encoded,
            options.jpeg_quality.unwrap_or(90),
        )
        .encode(
            &samples,
            raster.width(),
            raster.height(),
            image::ExtendedColorType::Rgb8,
        ),
    }
    .map_err(|e| PicError::image(e, ErrorCode::EncodeFailed))?;
    Ok(encoded)
}

/// Validate and resolve the parent before expensive work. A second check occurs at publication.
pub fn prepare_output(path: &Path, overwrite: bool) -> Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| PicError::new(ErrorCode::InvalidArgument, "output must name a file"))?;
    // JSON paths are exact, never lossy replacements of non-UTF-8 OS names.
    if path.to_str().is_none() {
        return Err(PicError::new(
            ErrorCode::InvalidArgument,
            "output path must be valid UTF-8",
        ));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::canonicalize(parent)
        .map_err(|e| PicError::io("resolve output directory", parent, e))?;
    if parent.to_str().is_none() {
        return Err(PicError::new(
            ErrorCode::InvalidArgument,
            "output directory must be valid UTF-8",
        ));
    }
    let destination = parent.join(name);
    match fs::symlink_metadata(&destination) {
        Ok(metadata) => {
            if !overwrite {
                return Err(PicError::new(
                    ErrorCode::OutputExists,
                    format!("'{}' exists; use --overwrite", destination.display()),
                ));
            }
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(PicError::new(
                    ErrorCode::InvalidArgument,
                    "overwrite requires a regular file, not a directory or symlink",
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(PicError::io("inspect output", &destination, e)),
    }
    Ok(destination)
}

/// Same-directory temporary file, completely written and closed before returning success.
/// No fsync durability guarantee; ordinary errors drop and remove the temporary file.
pub fn publish(destination: &Path, bytes: &[u8], overwrite: bool) -> Result<()> {
    publish_with(destination, overwrite, |file| file.write_all(bytes))
}

fn publish_with(
    destination: &Path,
    overwrite: bool,
    write: impl FnOnce(&mut fs::File) -> std::io::Result<()>,
) -> Result<()> {
    let destination = prepare_output(destination, overwrite)?;
    let parent = destination
        .parent()
        .ok_or_else(|| PicError::new(ErrorCode::InvalidArgument, "output has no parent"))?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".pic-")
        .tempfile_in(parent)
        .map_err(|e| PicError::io("create temporary output", parent, e))?;
    write(temporary.as_file_mut())
        .map_err(|e| PicError::io("write temporary output", &destination, e))?;
    temporary
        .flush()
        .map_err(|e| PicError::io("flush temporary output", &destination, e))?;
    let published = if overwrite {
        temporary.persist(&destination)
    } else {
        temporary.persist_noclobber(&destination)
    };
    let file = published.map_err(|e| PicError::io("publish output", &destination, e.error))?;
    drop(file);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        pub(super) static DECODE_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        pub(super) static ENCODE_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    #[test]
    fn multistep_file_run_decodes_and_encodes_once() {
        use crate::pipeline::{self, Pipeline, RunRequest};
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.png");
        let output = dir.path().join("output.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([120, 60, 20, 128]))
            .save(&input)
            .unwrap();
        let operations = serde_json::json!([
            {"op":"flip","op_version":1,"target":"canvas","params":{"axis":"horizontal"}},
            {"op":"adjust","op_version":1,"target":"canvas","params":{"exposure":1,"brightness":0,"contrast":1,"saturation":1}},
            {"op":"blur","op_version":1,"target":"canvas","params":{"sigma":1}},
            {"op":"sharpen","op_version":1,"target":"canvas","params":{"sigma":1,"amount":1}},
            {"op":"adjust","op_version":1,"target":"canvas","params":{"exposure":-1,"brightness":0,"contrast":1,"saturation":1}}
        ]);
        let limits = ResourceLimits::default();
        let pipeline = Pipeline::from_json(
            &serde_json::to_vec(&serde_json::json!({"schema_version":1,"operations":operations}))
                .unwrap(),
            dir.path(),
            &limits,
        )
        .unwrap();
        DECODE_CALLS.set(0);
        ENCODE_CALLS.set(0);
        let result = pipeline::run(
            RunRequest {
                input: &input,
                output: &output,
                pipeline: &pipeline,
                encoding: EncodeOptions::default(),
                overwrite: false,
            },
            &limits,
            &mut Diagnostics::default(),
        )
        .unwrap();
        assert_eq!(DECODE_CALLS.get(), 1);
        assert_eq!(ENCODE_CALLS.get(), 1);
        assert_eq!(result.operations_applied, 5);
        assert_eq!(
            image::open(output).unwrap().into_rgba8(),
            image::open(input).unwrap().into_rgba8()
        );
    }

    #[test]
    fn partial_write_failure_never_publishes_or_replaces_a_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("output.png");
        for overwrite in [false, true] {
            if overwrite {
                fs::write(&target, b"original").unwrap();
            }
            let error = publish_with(&target, overwrite, |file| {
                file.write_all(b"incomplete")?;
                Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "injected write failure",
                ))
            })
            .unwrap_err();
            assert_eq!(error.code, ErrorCode::IoError);
            if overwrite {
                assert_eq!(fs::read(&target).unwrap(), b"original");
            } else {
                assert!(!target.exists());
            }
            assert_eq!(
                fs::read_dir(dir.path()).unwrap().count(),
                usize::from(overwrite)
            );
        }
    }
}
