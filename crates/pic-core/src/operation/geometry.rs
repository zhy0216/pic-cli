//! Geometry v1: current-canvas coordinates, linear light, explicit sampling and alpha.

use std::str::FromStr;

use fast_image_resize::{
    FilterType, ResizeAlg, ResizeOptions, Resizer,
    images::{TypedImage, TypedImageRef},
    pixels::F32x4,
};
use serde::{Deserialize, Serialize};

use crate::{
    ErrorCode, PicError, Result, codec::srgb_to_linear, document::Raster, limits::ResourceLimits,
};

fn invalid(message: impl Into<String>) -> PicError {
    PicError::new(ErrorCode::InvalidArgument, message)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RgbaColor(pub [u8; 4]);

impl RgbaColor {
    pub fn linear(self) -> [f32; 4] {
        let [r, g, b, a] = self.0;
        [
            srgb_to_linear(r),
            srgb_to_linear(g),
            srgb_to_linear(b),
            f32::from(a) / 255.0,
        ]
    }
}

impl FromStr for RgbaColor {
    type Err = PicError;
    fn from_str(value: &str) -> Result<Self> {
        let value = value
            .strip_prefix('#')
            .ok_or_else(|| invalid("color requires #RRGGBB or #RRGGBBAA"))?;
        if ![6, 8].contains(&value.len()) || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid("color requires #RRGGBB or #RRGGBBAA"));
        }
        let mut channels = [255; 4];
        for (index, channel) in channels.iter_mut().take(value.len() / 2).enumerate() {
            *channel = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
                .map_err(|_| invalid("invalid hex color"))?;
        }
        Ok(Self(channels))
    }
}

macro_rules! string_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }
        impl FromStr for $name {
            type Err = PicError;
            fn from_str(value: &str) -> Result<Self> {
                serde_json::from_value(serde_json::Value::String(value.into()))
                    .map_err(|e| invalid(e.to_string()))
            }
        }
    };
}

string_enum!(Interpolation { Nearest, Bilinear });
string_enum!(FlipAxis {
    Horizontal,
    Vertical
});
string_enum!(Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight
});

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CropParams {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResizeParams {
    /// One absent dimension is inferred from the current aspect ratio, rounded half up.
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub filter: Interpolation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RotateParams {
    pub degrees: f64,
    pub expand: bool,
    pub filter: Interpolation,
    pub background: RgbaColor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlipParams {
    pub axis: FlipAxis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasParams {
    pub width: u32,
    pub height: u32,
    pub anchor: Anchor,
    pub background: RgbaColor,
}

fn positive(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(invalid("width and height must be positive"));
    }
    Ok(())
}

impl CropParams {
    pub fn validate(&self) -> Result<()> {
        positive(self.width, self.height)?;
        if self.x.checked_add(self.width).is_none() || self.y.checked_add(self.height).is_none() {
            return Err(invalid("crop rectangle overflows pixel coordinates"));
        }
        Ok(())
    }
}

impl ResizeParams {
    pub fn validate(&self) -> Result<()> {
        if self.width.is_none() && self.height.is_none() {
            return Err(invalid("resize requires width or height"));
        }
        positive(self.width.unwrap_or(1), self.height.unwrap_or(1))
    }

    pub fn dimensions(&self, width: u32, height: u32) -> Result<(u32, u32)> {
        self.validate()?;
        positive(width, height)?;
        let scaled = |requested: u32, numerator: u32, denominator: u32| {
            // u32*u32 + u32/2 fits u64. Clamp an inferred subpixel dimension to one.
            let value = ((u64::from(requested) * u64::from(numerator)
                + u64::from(denominator) / 2)
                / u64::from(denominator))
            .max(1);
            u32::try_from(value).map_err(|_| {
                PicError::new(
                    ErrorCode::ResourceLimit,
                    "inferred resize dimension overflows u32",
                )
            })
        };
        match (self.width, self.height) {
            (Some(w), Some(h)) => Ok((w, h)),
            (Some(w), None) => Ok((w, scaled(w, height, width)?)),
            (None, Some(h)) => Ok((scaled(h, width, height)?, h)),
            (None, None) => unreachable!("validated above"),
        }
    }
}

impl RotateParams {
    pub fn validate(&self) -> Result<()> {
        if !self.degrees.is_finite() || !(-360.0..=360.0).contains(&self.degrees) {
            return Err(invalid(
                "rotation degrees must be finite and in [-360, 360]",
            ));
        }
        Ok(())
    }
}

impl CanvasParams {
    pub fn validate(&self) -> Result<()> {
        positive(self.width, self.height)
    }
}

/// Include simultaneously live source/destination and sampling scratch in admission.
fn buffers(limits: &ResourceLimits, counts: &[u64], extra_bytes: u64) -> Result<()> {
    let bytes = counts.iter().try_fold(extra_bytes, |sum, count| {
        sum.checked_add(count.checked_mul(16)?)
    });
    if bytes.is_none_or(|bytes| bytes > limits.max_buffer_bytes || usize::try_from(bytes).is_err())
    {
        return Err(PicError::new(
            ErrorCode::ResourceLimit,
            "geometry buffers exceed memory or address-space limit",
        ));
    }
    Ok(())
}

fn pixels(count: usize) -> Result<Vec<[f32; 4]>> {
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(count)
        .map_err(|_| PicError::new(ErrorCode::ResourceLimit, "cannot allocate geometry pixels"))?;
    Ok(pixels)
}

fn mapped(
    raster: &Raster,
    width: u32,
    height: u32,
    limits: &ResourceLimits,
    mut sample: impl FnMut(u32, u32) -> [f32; 4],
) -> Result<Raster> {
    let count = limits.check_dimensions(width, height)?;
    buffers(limits, &[raster.pixels().len() as u64, count as u64], 0)?;
    let mut output = pixels(count)?;
    for y in 0..height {
        for x in 0..width {
            output.push(sample(x, y));
        }
    }
    Raster::from_linear_rgba(width, height, output, limits)
}

fn at(raster: &Raster, x: u32, y: u32) -> [f32; 4] {
    raster.pixels()[y as usize * raster.width() as usize + x as usize]
}

pub(super) fn crop(raster: &Raster, p: &CropParams, limits: &ResourceLimits) -> Result<Raster> {
    if p.x + p.width > raster.width() || p.y + p.height > raster.height() {
        return Err(invalid(
            "crop rectangle must lie entirely inside the current canvas",
        ));
    }
    mapped(raster, p.width, p.height, limits, |x, y| {
        at(raster, x + p.x, y + p.y)
    })
}

pub(super) fn flip(raster: &Raster, p: &FlipParams, limits: &ResourceLimits) -> Result<Raster> {
    mapped(
        raster,
        raster.width(),
        raster.height(),
        limits,
        |x, y| match p.axis {
            FlipAxis::Horizontal => at(raster, raster.width() - 1 - x, y),
            FlipAxis::Vertical => at(raster, x, raster.height() - 1 - y),
        },
    )
}

pub(super) fn canvas(raster: &Raster, p: &CanvasParams, limits: &ResourceLimits) -> Result<Raster> {
    use Anchor::*;
    let dx = i64::from(p.width) - i64::from(raster.width());
    let dy = i64::from(p.height) - i64::from(raster.height());
    let left = match p.anchor {
        TopLeft | Left | BottomLeft => 0,
        Top | Center | Bottom => dx.div_euclid(2),
        TopRight | Right | BottomRight => dx,
    };
    let top = match p.anchor {
        TopLeft | Top | TopRight => 0,
        Left | Center | Right => dy.div_euclid(2),
        BottomLeft | Bottom | BottomRight => dy,
    };
    let background = p.background.linear();
    mapped(raster, p.width, p.height, limits, |x, y| {
        outside(raster, i64::from(x) - left, i64::from(y) - top, background)
    })
}

pub(super) fn resize(raster: &Raster, p: &ResizeParams, limits: &ResourceLimits) -> Result<Raster> {
    let (width, height) = p.dimensions(raster.width(), raster.height())?;
    let count = limits.check_dimensions(width, height)?;
    if (width, height) == (raster.width(), raster.height()) {
        return Ok(raster.clone());
    }
    if p.filter == Interpolation::Nearest {
        // Exact rational center mapping; ties select the pixel to the right/bottom.
        return mapped(raster, width, height, limits, |x, y| {
            let sx = ((2 * u128::from(x) + 1) * u128::from(raster.width())
                / (2 * u128::from(width))) as u32;
            let sy = ((2 * u128::from(y) + 1) * u128::from(raster.height())
                / (2 * u128::from(height))) as u32;
            at(raster, sx, sy)
        });
    }
    // Conservative bound for either convolution pass order plus coefficient tables.
    let scratch = (u64::from(width) * u64::from(raster.height()))
        .max(u64::from(raster.width()) * u64::from(height));
    let coefficient_bytes = 64
        * (u64::from(width)
            + u64::from(height)
            + u64::from(raster.width())
            + u64::from(raster.height()))
        + 16;
    buffers(
        limits,
        &[
            raster.pixels().len() as u64,
            raster.pixels().len() as u64,
            count as u64,
            scratch,
        ],
        coefficient_bytes,
    )?;
    let mut source = pixels(raster.pixels().len())?;
    source.extend(
        raster
            .pixels()
            .iter()
            .map(|p| [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]]),
    );
    let mut output = pixels(count)?;
    output.resize(count, [0.0; 4]);
    let source = TypedImageRef::<F32x4>::new(
        raster.width(),
        raster.height(),
        bytemuck::cast_slice(&source),
    )
    .map_err(|e| invalid(e.to_string()))?;
    let mut destination = TypedImage::<F32x4>::from_pixels_slice(
        width,
        height,
        bytemuck::cast_slice_mut(&mut output),
    )
    .map_err(|e| invalid(e.to_string()))?;
    Resizer::new()
        .resize_typed(
            &source,
            &mut destination,
            &ResizeOptions::new()
                .resize_alg(ResizeAlg::Convolution(FilterType::Bilinear))
                .use_alpha(false),
        )
        .map_err(|e| invalid(format!("resize failed: {e}")))?;
    for pixel in &mut output {
        let alpha = pixel[3];
        if alpha > 0.0 {
            for value in &mut pixel[..3] {
                *value /= alpha;
            }
            pixel[3] = alpha.clamp(0.0, 1.0);
        } else {
            *pixel = [0.0; 4];
        }
    }
    Raster::from_linear_rgba(width, height, output, limits)
}

fn outside(raster: &Raster, x: i64, y: i64, background: [f32; 4]) -> [f32; 4] {
    if x >= 0 && y >= 0 && x < i64::from(raster.width()) && y < i64::from(raster.height()) {
        at(raster, x as u32, y as u32)
    } else {
        background
    }
}

fn sample(
    raster: &Raster,
    x: f64,
    y: f64,
    filter: Interpolation,
    background: [f32; 4],
) -> [f32; 4] {
    if filter == Interpolation::Nearest {
        return outside(raster, x.floor() as i64, y.floor() as i64, background);
    }
    let x = x - 0.5;
    let y = y - 0.5;
    let (left, top) = (x.floor() as i64, y.floor() as i64);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let mut result = [0.0f64; 4];
    for (dy, wy) in [(0, 1.0 - fy), (1, fy)] {
        for (dx, wx) in [(0, 1.0 - fx), (1, fx)] {
            let p = outside(raster, left + dx, top + dy, background);
            let alpha_weight = f64::from(p[3]) * wx * wy;
            for channel in 0..3 {
                result[channel] += f64::from(p[channel]) * alpha_weight;
            }
            result[3] += alpha_weight;
        }
    }
    let alpha = result[3];
    if alpha <= 0.0 {
        return [0.0; 4];
    }
    [
        (result[0] / alpha) as f32,
        (result[1] / alpha) as f32,
        (result[2] / alpha) as f32,
        alpha.clamp(0.0, 1.0) as f32,
    ]
}

pub(super) fn rotate(raster: &Raster, p: &RotateParams, limits: &ResourceLimits) -> Result<Raster> {
    let angle = p.degrees.rem_euclid(360.0);
    let (sw, sh) = (raster.width(), raster.height());
    // Exact orthogonal transforms preserve HDR, signed zero and hidden transparent RGB.
    if angle == 0.0 {
        return Ok(raster.clone());
    }
    if angle == 180.0 {
        return mapped(raster, sw, sh, limits, |x, y| {
            at(raster, sw - 1 - x, sh - 1 - y)
        });
    }
    if (p.expand || sw == sh) && (angle == 90.0 || angle == 270.0) {
        return mapped(raster, sh, sw, limits, |x, y| {
            if angle == 90.0 {
                at(raster, y, sh - 1 - x)
            } else {
                at(raster, sw - 1 - y, x)
            }
        });
    }
    let (sin, cos) = if angle == 90.0 {
        (1.0, 0.0)
    } else if angle == 270.0 {
        (-1.0, 0.0)
    } else {
        angle.to_radians().sin_cos()
    };
    let (width, height) = if p.expand {
        let w = (f64::from(sw) * cos.abs() + f64::from(sh) * sin.abs()).ceil();
        let h = (f64::from(sw) * sin.abs() + f64::from(sh) * cos.abs()).ceil();
        if w > f64::from(u32::MAX) || h > f64::from(u32::MAX) {
            return Err(PicError::new(
                ErrorCode::ResourceLimit,
                "rotated dimensions overflow u32",
            ));
        }
        (w as u32, h as u32)
    } else {
        (sw, sh)
    };
    let background = p.background.linear();
    mapped(raster, width, height, limits, |x, y| {
        let dx = f64::from(x) + 0.5 - f64::from(width) / 2.0;
        let dy = f64::from(y) + 0.5 - f64::from(height) / 2.0;
        sample(
            raster,
            cos * dx + sin * dy + f64::from(sw) / 2.0,
            -sin * dx + cos * dy + f64::from(sh) / 2.0,
            p.filter,
            background,
        )
    })
}
