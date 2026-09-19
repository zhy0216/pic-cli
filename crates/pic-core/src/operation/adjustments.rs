//! Photo operations v1: scalar linear-light math, no intermediate gamut clipping.
//! See docs/adjustments-filters.md for equations, ranges, endpoints and alpha semantics.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{ErrorCode, PicError, Result, document::Raster, limits::ResourceLimits};

fn invalid(message: impl Into<String>) -> PicError {
    PicError::new(ErrorCode::InvalidArgument, message)
}

fn range(name: &str, value: f64, min: f64, max: f64) -> Result<()> {
    if !value.is_finite() || !(min..=max).contains(&value) {
        return Err(invalid(format!(
            "{name} must be finite and in [{min}, {max}]"
        )));
    }
    Ok(())
}

/// RGB means the same scalar mapping applied independently to all three color channels.
/// Alpha is never a selectable color channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Rgb,
    Red,
    Green,
    Blue,
}

impl FromStr for Channel {
    type Err = PicError;
    fn from_str(value: &str) -> Result<Self> {
        serde_json::from_value(serde_json::Value::String(value.into()))
            .map_err(|e| invalid(e.to_string()))
    }
}

impl Channel {
    fn includes(self, index: usize) -> bool {
        self == Self::Rgb
            || index
                == match self {
                    Self::Red => 0,
                    Self::Green => 1,
                    Self::Blue => 2,
                    Self::Rgb => unreachable!(),
                }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdjustParams {
    pub exposure: f64,
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
}

impl AdjustParams {
    pub fn validate(&self) -> Result<()> {
        range("exposure EV", self.exposure, -20.0, 20.0)?;
        range("brightness", self.brightness, -1.0, 1.0)?;
        range("contrast", self.contrast, 0.0, 10.0)?;
        range("saturation", self.saturation, 0.0, 10.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelsParams {
    pub channel: Channel,
    pub input_black: f64,
    pub input_white: f64,
    pub gamma: f64,
    pub output_black: f64,
    pub output_white: f64,
}

impl LevelsParams {
    pub fn validate(&self) -> Result<()> {
        range("input_black", self.input_black, 0.0, 1.0)?;
        range("input_white", self.input_white, 0.0, 1.0)?;
        range("gamma", self.gamma, 0.1, 10.0)?;
        range("output_black", self.output_black, 0.0, 1.0)?;
        range("output_white", self.output_white, 0.0, 1.0)?;
        if self.input_black >= self.input_white {
            return Err(invalid("input_black must be less than input_white"));
        }
        if self.output_black > self.output_white {
            return Err(invalid("output_black must not exceed output_white"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurvesParams {
    pub channel: Channel,
    pub points: Vec<[f64; 2]>,
}

impl CurvesParams {
    pub fn validate(&self) -> Result<()> {
        if !(2..=256).contains(&self.points.len()) {
            return Err(invalid("curves requires 2..256 [input, output] points"));
        }
        for [x, y] in &self.points {
            range("curve input", *x, 0.0, 1.0)?;
            range("curve output", *y, 0.0, 1.0)?;
        }
        if self.points[0][0] != 0.0 || self.points[self.points.len() - 1][0] != 1.0 {
            return Err(invalid("curve inputs must start at 0 and end at 1"));
        }
        if self.points.windows(2).any(|pair| pair[0][0] >= pair[1][0]) {
            return Err(invalid("curve inputs must be strictly increasing"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlurParams {
    /// Gaussian standard deviation in pixels; zero is exact identity.
    pub sigma: f64,
}

impl BlurParams {
    pub fn validate(&self) -> Result<()> {
        range("sigma", self.sigma, 0.0, 100.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharpenParams {
    pub sigma: f64,
    pub amount: f64,
}

impl SharpenParams {
    pub fn validate(&self) -> Result<()> {
        range("sigma", self.sigma, 0.0, 100.0)?;
        range("amount", self.amount, 0.0, 10.0)
    }
}

fn allocate<T>(count: usize) -> Result<Vec<T>> {
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| {
        PicError::new(
            ErrorCode::ResourceLimit,
            "cannot allocate photo operation buffer",
        )
    })?;
    Ok(values)
}

fn buffers(
    raster: &Raster,
    bytes_per_pixel: u64,
    extra: u64,
    limits: &ResourceLimits,
) -> Result<()> {
    let bytes = (raster.pixels().len() as u64)
        .checked_mul(bytes_per_pixel)
        .and_then(|bytes| bytes.checked_add(extra));
    if bytes.is_none_or(|bytes| bytes > limits.max_buffer_bytes || usize::try_from(bytes).is_err())
    {
        return Err(PicError::new(
            ErrorCode::ResourceLimit,
            "photo operation buffers exceed memory or address-space limit",
        ));
    }
    Ok(())
}

fn mapped(
    raster: &Raster,
    limits: &ResourceLimits,
    mut transform: impl FnMut([f32; 4]) -> [f32; 4],
) -> Result<Raster> {
    buffers(raster, 32, 0, limits)?;
    let mut output = allocate(raster.pixels().len())?;
    output.extend(raster.pixels().iter().copied().map(&mut transform));
    Raster::from_linear_rgba(raster.width(), raster.height(), output, limits)
}

fn luminance(rgb: [f64; 3]) -> f64 {
    // Equivalent Rec.709 weights, expressed around green to preserve neutral grays.
    rgb[1] + 0.2126 * (rgb[0] - rgb[1]) + 0.0722 * (rgb[2] - rgb[1])
}

pub(super) fn adjust(raster: &Raster, p: &AdjustParams, limits: &ResourceLimits) -> Result<Raster> {
    if p.exposure == 0.0 && p.brightness == 0.0 && p.contrast == 1.0 && p.saturation == 1.0 {
        return Ok(raster.clone());
    }
    let exposure = p.exposure.exp2();
    mapped(raster, limits, |mut pixel| {
        let mut rgb = [
            f64::from(pixel[0]),
            f64::from(pixel[1]),
            f64::from(pixel[2]),
        ];
        for value in &mut rgb {
            if p.exposure != 0.0 {
                *value *= exposure;
            }
            if p.brightness != 0.0 {
                *value += p.brightness;
            }
            if p.contrast != 1.0 {
                *value = (*value - 0.5) * p.contrast + 0.5;
            }
        }
        if p.saturation != 1.0 {
            let gray = luminance(rgb);
            for value in &mut rgb {
                *value = gray + p.saturation * (*value - gray);
            }
        }
        for index in 0..3 {
            pixel[index] = rgb[index] as f32;
        }
        pixel
    })
}

pub(super) fn levels(raster: &Raster, p: &LevelsParams, limits: &ResourceLimits) -> Result<Raster> {
    if p.gamma == 1.0 && p.input_black == p.output_black && p.input_white == p.output_white {
        return Ok(raster.clone());
    }
    mapped(raster, limits, |mut pixel| {
        for (index, value) in pixel[..3].iter_mut().enumerate() {
            if p.channel.includes(index) {
                // A collapsed output range is constant, even for extreme finite inputs.
                *value = if p.output_black == p.output_white {
                    p.output_black as f32
                } else {
                    let t = (f64::from(*value) - p.input_black) / (p.input_white - p.input_black);
                    let corrected = if p.gamma == 1.0 {
                        t
                    } else {
                        t.abs().powf(1.0 / p.gamma).copysign(t)
                    };
                    (p.output_black + (p.output_white - p.output_black) * corrected) as f32
                };
            }
        }
        pixel
    })
}

pub(super) fn curves(raster: &Raster, p: &CurvesParams, limits: &ResourceLimits) -> Result<Raster> {
    if p.points.iter().all(|[x, y]| x == y) {
        return Ok(raster.clone());
    }
    mapped(raster, limits, |mut pixel| {
        for (index, value) in pixel[..3].iter_mut().enumerate() {
            if p.channel.includes(index) {
                let x = f64::from(*value);
                let upper = p.points.partition_point(|point| point[0] < x);
                // Interpolate inside the domain, extend the first/last segment outside.
                let lower = upper.saturating_sub(1).min(p.points.len() - 2);
                let [x0, y0] = p.points[lower];
                let [x1, y1] = p.points[lower + 1];
                *value = if x == x0 || y0 == y1 {
                    y0 as f32
                } else if x == x1 {
                    y1 as f32
                } else {
                    (y0 + (x - x0) * ((y1 - y0) / (x1 - x0))) as f32
                };
            }
        }
        pixel
    })
}

pub(super) fn grayscale(raster: &Raster, limits: &ResourceLimits) -> Result<Raster> {
    mapped(raster, limits, |pixel| {
        let gray = luminance([
            f64::from(pixel[0]),
            f64::from(pixel[1]),
            f64::from(pixel[2]),
        ]) as f32;
        [gray, gray, gray, pixel[3]]
    })
}

pub(super) fn invert(raster: &Raster, limits: &ResourceLimits) -> Result<Raster> {
    mapped(raster, limits, |pixel| {
        [
            (1.0 - f64::from(pixel[0])) as f32,
            (1.0 - f64::from(pixel[1])) as f32,
            (1.0 - f64::from(pixel[2])) as f32,
            pixel[3],
        ]
    })
}

pub(super) fn blur(raster: &Raster, sigma: f64, limits: &ResourceLimits) -> Result<Raster> {
    if sigma == 0.0 {
        return Ok(raster.clone());
    }
    let radius = (3.0 * sigma).ceil() as i64;
    let taps = (2 * radius + 1) as usize;
    // Original RGBA32F + horizontal premultiplied RGBA64F + final RGBA32F + kernel.
    buffers(raster, 64, taps as u64 * 8, limits)?;
    let mut kernel = allocate::<f64>(taps)?;
    for offset in -radius..=radius {
        // Divide before squaring: tiny positive sigma must not make the center 0/0.
        let distance = offset as f64 / sigma;
        kernel.push((-0.5 * distance * distance).exp());
    }
    let total: f64 = kernel.iter().sum();
    for weight in &mut kernel {
        *weight /= total;
    }
    let (width, height) = (raster.width() as usize, raster.height() as usize);
    let mut horizontal = allocate::<[f64; 4]>(raster.pixels().len())?;
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0.0; 4];
            for (tap, weight) in kernel.iter().enumerate() {
                let sx = (x as i64 + tap as i64 - radius).clamp(0, width as i64 - 1) as usize;
                let pixel = raster.pixels()[y * width + sx];
                let alpha_weight = f64::from(pixel[3]) * weight;
                for channel in 0..3 {
                    sum[channel] += f64::from(pixel[channel]) * alpha_weight;
                }
                sum[3] += alpha_weight;
            }
            horizontal.push(sum);
        }
    }
    let mut output = allocate(raster.pixels().len())?;
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0.0; 4];
            for (tap, weight) in kernel.iter().enumerate() {
                let sy = (y as i64 + tap as i64 - radius).clamp(0, height as i64 - 1) as usize;
                let pixel = horizontal[sy * width + x];
                for channel in 0..4 {
                    sum[channel] += pixel[channel] * weight;
                }
            }
            let alpha = sum[3].clamp(0.0, 1.0) as f32;
            output.push(if alpha == 0.0 {
                [0.0; 4]
            } else {
                [
                    (sum[0] / sum[3]) as f32,
                    (sum[1] / sum[3]) as f32,
                    (sum[2] / sum[3]) as f32,
                    alpha,
                ]
            });
        }
    }
    Raster::from_linear_rgba(raster.width(), raster.height(), output, limits)
}

pub(super) fn sharpen(
    raster: &Raster,
    p: &SharpenParams,
    limits: &ResourceLimits,
) -> Result<Raster> {
    if p.sigma == 0.0 || p.amount == 0.0 {
        return Ok(raster.clone());
    }
    let blurred = blur(raster, p.sigma, limits)?;
    buffers(raster, 48, 0, limits)?;
    let mut output = allocate(raster.pixels().len())?;
    for (source, blurred) in raster.pixels().iter().zip(blurred.pixels()) {
        let mut pixel = *source;
        // Preserve invisible color; it cannot contribute to the visible blur reference.
        if source[3] > 0.0 {
            for channel in 0..3 {
                let value = f64::from(source[channel]);
                pixel[channel] = (value + p.amount * (value - f64::from(blurred[channel]))) as f32;
            }
        }
        output.push(pixel);
    }
    Raster::from_linear_rgba(raster.width(), raster.height(), output, limits)
}
