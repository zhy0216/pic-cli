//! Shared linear-light source-over compositor. Coordinates are continuous pixel edges.
use serde::{Deserialize, Serialize};

use crate::{ErrorCode, PicError, Result, document::Layer, operation::geometry::Interpolation};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
}

impl std::str::FromStr for BlendMode {
    type Err = PicError;
    fn from_str(value: &str) -> Result<Self> {
        serde_json::from_value(serde_json::Value::String(value.into()))
            .map_err(|e| PicError::new(ErrorCode::InvalidArgument, e.to_string()))
    }
}

/// x'=a*x+c*y+e; y'=b*x+d*y+f. No integer rounding in coordinate conversion.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Affine(pub [f64; 6]);

impl Affine {
    pub fn map(self, [x, y]: [f64; 2]) -> [f64; 2] {
        let [a, b, c, d, e, f] = self.0;
        [a * x + c * y + e, b * x + d * y + f]
    }
    pub fn inverse(self) -> Self {
        let [a, b, c, d, e, f] = self.0;
        let det = a * d - b * c;
        Self([
            d / det,
            -b / det,
            -c / det,
            a / det,
            (c * f - d * e) / det,
            (b * e - a * f) / det,
        ])
    }
    /// Reject overflow/underflow in the determinant before accepting inverse coordinates.
    /// Finite matrix entries alone do not imply a numerically representable inverse.
    pub(crate) fn checked_inverse(self) -> Option<Self> {
        let [a, b, c, d, _, _] = self.0;
        let determinant = a * d - b * c;
        if !self.0.iter().all(|v| v.is_finite()) || !determinant.is_finite() || determinant == 0.0 {
            return None;
        }
        let inverse = self.inverse();
        if !inverse.0.iter().all(|v| v.is_finite()) {
            return None;
        }
        let roundtrip = self.then(inverse);
        if roundtrip.0[..4]
            .iter()
            .zip([1.0, 0.0, 0.0, 1.0])
            .any(|(actual, expected)| !actual.is_finite() || (actual - expected).abs() > 1e-8)
        {
            return None;
        }
        Some(inverse)
    }
    /// Apply `other` first, then self.
    pub fn then(self, other: Self) -> Self {
        let [a, b, c, d, e, f] = self.0;
        let [g, h, i, j, k, l] = other.0;
        Self([
            a * g + c * h,
            b * g + d * h,
            a * i + c * j,
            b * i + d * j,
            a * k + c * l + e,
            b * k + d * l + f,
        ])
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transform {
    pub x: f64,
    pub y: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub degrees: f64,
    pub flip_x: bool,
    pub flip_y: bool,
    pub filter: Interpolation,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            degrees: 0.0,
            flip_x: false,
            flip_y: false,
            filter: Interpolation::Bilinear,
        }
    }
}

impl Transform {
    pub fn validate(&self) -> Result<()> {
        if ![self.x, self.y, self.scale_x, self.scale_y, self.degrees]
            .iter()
            .all(|v| v.is_finite())
            || self.x.abs() > 1e9
            || self.y.abs() > 1e9
            || !(1e-6..=1e6).contains(&self.scale_x)
            || !(1e-6..=1e6).contains(&self.scale_y)
            || !(-360.0..=360.0).contains(&self.degrees)
        {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "transform requires finite position within +/-1e9, scales in [1e-6,1e6], degrees in [-360,360]",
            ));
        }
        Ok(())
    }

    /// Flip about local bounds, scale, rotate clockwise about local origin, then translate.
    pub fn matrix(&self, width: u32, height: u32) -> Affine {
        let (sin, cos) = match self.degrees.rem_euclid(360.0) {
            0.0 => (0.0, 1.0),
            90.0 => (1.0, 0.0),
            180.0 => (0.0, -1.0),
            270.0 => (-1.0, 0.0),
            angle => angle.to_radians().sin_cos(),
        };
        let sx = self.scale_x * if self.flip_x { -1.0 } else { 1.0 };
        let sy = self.scale_y * if self.flip_y { -1.0 } else { 1.0 };
        let dx = if self.flip_x {
            f64::from(width) * self.scale_x
        } else {
            0.0
        };
        let dy = if self.flip_y {
            f64::from(height) * self.scale_y
        } else {
            0.0
        };
        Affine([
            cos * sx,
            sin * sx,
            -sin * sy,
            cos * sy,
            self.x + cos * dx - sin * dy,
            self.y + sin * dx + cos * dy,
        ])
    }
}

/// Source-over with separable blend B, using the W3C overlap/non-overlap equation.
/// Finite HDR RGB is retained; alpha/opacity alone are bounded. Hidden RGB cannot bleed.
pub fn blend(back: [f32; 4], front: [f32; 4], opacity: f64, mode: BlendMode) -> [f32; 4] {
    let ab = f64::from(back[3]);
    let asrc = f64::from(front[3]) * opacity;
    if asrc == 0.0 {
        return back;
    }
    if ab == 0.0 {
        return [front[0], front[1], front[2], asrc as f32];
    }
    let alpha = asrc + ab * (1.0 - asrc);
    let mut out = [0.0; 4];
    for c in 0..3 {
        let b = f64::from(back[c]);
        let s = f64::from(front[c]);
        let mixed = match mode {
            BlendMode::Normal => s,
            BlendMode::Multiply => b * s,
            BlendMode::Screen => b + s - b * s,
            BlendMode::Overlay => {
                if b <= 0.5 {
                    2.0 * b * s
                } else {
                    1.0 - 2.0 * (1.0 - b) * (1.0 - s)
                }
            }
        };
        out[c] =
            ((asrc * (1.0 - ab) * s + asrc * ab * mixed + (1.0 - asrc) * ab * b) / alpha) as f32;
    }
    out[3] = alpha as f32;
    out
}

/// Mask coverage is multiplied into each source tap BEFORE premultiplied filtering.
/// Sampling the color and coverage separately would allow masked colors to bleed.
pub(crate) fn sample(layer: &Layer, point: [f64; 2], mask_only: bool) -> [f32; 4] {
    let tap = |x: i64, y: i64| {
        if x < 0
            || y < 0
            || x >= i64::from(layer.raster.width())
            || y >= i64::from(layer.raster.height())
        {
            return [0.0; 4];
        }
        let i = y as usize * layer.raster.width() as usize + x as usize;
        let mask = layer.mask.as_ref().map_or(1.0, |mask| mask.pixels()[i][3]);
        if mask_only {
            return [1.0, 1.0, 1.0, mask];
        }
        let mut p = layer.raster.pixels()[i];
        p[3] *= mask;
        p
    };
    let [x, y] = point;
    if !x.is_finite()
        || !y.is_finite()
        || x < -1.0
        || y < -1.0
        || x > f64::from(layer.raster.width()) + 1.0
        || y > f64::from(layer.raster.height()) + 1.0
    {
        return [0.0; 4];
    }
    if layer.transform.filter == Interpolation::Nearest {
        return tap(x.floor() as i64, y.floor() as i64);
    }
    let (x, y) = (x - 0.5, y - 0.5);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let mut out = [0.0f64; 4];
    for (dy, wy) in [(0, 1.0 - fy), (1, fy)] {
        for (dx, wx) in [(0, 1.0 - fx), (1, fx)] {
            let p = tap(x.floor() as i64 + dx, y.floor() as i64 + dy);
            let weight = f64::from(p[3]) * wx * wy;
            for c in 0..3 {
                out[c] += f64::from(p[c]) * weight;
            }
            out[3] += weight;
        }
    }
    if out[3] <= 0.0 {
        return [0.0; 4];
    }
    [
        (out[0] / out[3]) as f32,
        (out[1] / out[3]) as f32,
        (out[2] / out[3]) as f32,
        out[3].clamp(0.0, 1.0) as f32,
    ]
}

/// Visualize a coverage carrier only after transforming/cropping/resizing its alpha.
pub(crate) fn visualize_coverage(
    raster: &crate::document::Raster,
    limits: &crate::limits::ResourceLimits,
) -> Result<crate::document::Raster> {
    let count = limits.check_dimensions(raster.width(), raster.height())?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(count)
        .map_err(|_| PicError::new(ErrorCode::ResourceLimit, "cannot allocate mask preview"))?;
    pixels.extend(raster.pixels().iter().map(|p| {
        let coverage = p[3];
        let gray = if coverage <= 0.04045 {
            coverage / 12.92
        } else {
            ((coverage + 0.055) / 1.055).powf(2.4)
        };
        [gray, gray, gray, 1.0]
    }));
    crate::document::Raster::from_linear_rgba(raster.width(), raster.height(), pixels, limits)
}
