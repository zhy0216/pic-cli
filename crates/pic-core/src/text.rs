//! Explicit-font horizontal layout. No system lookup, fallback, or implicit wrapping.
use ab_glyph::{Font, FontRef, Glyph, GlyphId, PxScale, point};
use serde::{Deserialize, Serialize};
use unicode_script::{Script, UnicodeScript};

use crate::{
    ErrorCode, PicError, Result,
    composite::{BlendMode, blend},
    document::Raster,
    limits::ResourceLimits,
    operation::geometry::RgbaColor,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

impl std::str::FromStr for TextAlign {
    type Err = PicError;
    fn from_str(value: &str) -> Result<Self> {
        serde_json::from_value(serde_json::json!(value))
            .map_err(|e| PicError::new(ErrorCode::InvalidArgument, e.to_string()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextParams {
    pub text: String,
    /// Path in authoring ops; content-addressed asset in committed ops.
    pub font: String,
    /// Pixels per em, independent of DPI.
    pub size: f32,
    pub line_height: f32,
    pub width: u32,
    pub height: u32,
    pub align: TextAlign,
    pub color: RgbaColor,
}

impl TextParams {
    pub fn validate(&self) -> Result<()> {
        if self.font.is_empty()
            || self.text.len() > 65_536
            || !self.size.is_finite()
            || !(1.0..=512.0).contains(&self.size)
            || !self.line_height.is_finite()
            || !(1.0..=4096.0).contains(&self.line_height)
            || self.width == 0
            || self.height == 0
        {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "text needs an explicit font, <=65536 UTF-8 bytes, size 1..512 px/em, line_height 1..4096 px and positive box dimensions",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PositionedGlyph {
    pub id: u16,
    pub cluster: u32,
    pub x: f32,
    pub baseline: f32,
}

#[derive(Debug, Clone)]
pub struct TextLayout {
    pub glyphs: Vec<PositionedGlyph>,
    pub line_widths: Vec<f32>,
}

fn invalid_font(message: &str) -> PicError {
    PicError::new(ErrorCode::InvalidFont, message)
}

fn face(bytes: &[u8]) -> Result<rustybuzz::Face<'_>> {
    let face = rustybuzz::Face::from_slice(bytes, 0).ok_or_else(|| {
        invalid_font("cannot parse font; expected a static TrueType outline font")
    })?;
    let raw = face.raw_face();
    if bytes.starts_with(b"ttcf")
        || face.is_variable()
        || raw
            .table(rustybuzz::ttf_parser::Tag::from_bytes(b"glyf"))
            .is_none()
        || [b"COLR", b"CBDT", b"sbix", b"SVG "].iter().any(|tag| {
            raw.table(rustybuzz::ttf_parser::Tag::from_bytes(tag))
                .is_some()
        })
    {
        return Err(invalid_font(
            "only static, single-face TrueType outlines are supported; collections, variable, color and bitmap fonts are rejected",
        ));
    }
    Ok(face)
}

/// Rustybuzz applies default OpenType substitutions/positioning (including liga/kern/marks).
/// Each LF-delimited line accepts one of Latin/Greek/Cyrillic plus Common/Inherited characters.
pub fn layout(params: &TextParams, bytes: &[u8]) -> Result<TextLayout> {
    params.validate()?;
    let face = face(bytes)?;
    let scale = params.size / face.units_per_em() as f32;
    let ascent = f32::from(face.ascender()) * scale;
    let mut glyphs = Vec::new();
    let mut line_widths = Vec::new();
    let mut byte_offset = 0;
    for (line_number, line) in params.text.split('\n').enumerate() {
        let mut script = None;
        for (offset, ch) in line.char_indices() {
            if ch.is_control()
                || matches!(ch as u32, 0x200b..=0x200f | 0x2028..=0x202e | 0x2060..=0x206f | 0xfe00..=0xfe0f | 0xe0100..=0xe01ef)
            {
                return Err(PicError::new(
                    ErrorCode::UnsupportedText,
                    format!(
                        "unsupported text control U+{:04X} at byte {}; use LF for explicit line breaks",
                        ch as u32,
                        byte_offset + offset
                    ),
                ));
            }
            if face.glyph_index(ch).is_none_or(|id| id.0 == 0) {
                return Err(PicError::new(
                    ErrorCode::MissingGlyph,
                    format!(
                        "font '{}' has no glyph for U+{:04X} at byte {}",
                        params.font,
                        ch as u32,
                        byte_offset + offset
                    ),
                ));
            }
            match ch.script() {
                Script::Common | Script::Inherited => (),
                s @ (Script::Latin | Script::Greek | Script::Cyrillic) => {
                    if script.is_some_and(|old| old != s) {
                        return Err(PicError::new(
                            ErrorCode::UnsupportedText,
                            "use one supported alphabet per line; mixed-script itemization is not supported",
                        ));
                    }
                    script = Some(s);
                }
                _ => {
                    return Err(PicError::new(
                        ErrorCode::UnsupportedText,
                        "supported scripts are Latin, Greek and Cyrillic (horizontal LTR); CJK, bidi and complex-script layout are not supported",
                    ));
                }
            }
        }
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(line);
        buffer.set_direction(rustybuzz::Direction::LeftToRight);
        buffer.guess_segment_properties();
        let shaped = rustybuzz::shape(&face, &[], buffer);
        let width = shaped
            .glyph_positions()
            .iter()
            .map(|p| p.x_advance as f32 * scale)
            .sum::<f32>();
        line_widths.push(width);
        let mut x = match params.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => (params.width as f32 - width) / 2.0,
            TextAlign::Right => params.width as f32 - width,
        };
        let mut baseline = ascent + line_number as f32 * params.line_height;
        for (info, position) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
            if info.glyph_id == 0 {
                return Err(PicError::new(
                    ErrorCode::MissingGlyph,
                    format!(
                        "shaping produced a missing glyph at byte {}",
                        byte_offset + info.cluster as usize
                    ),
                ));
            }
            glyphs.push(PositionedGlyph {
                id: info.glyph_id as u16,
                cluster: byte_offset as u32 + info.cluster,
                x: x + position.x_offset as f32 * scale,
                baseline: baseline - position.y_offset as f32 * scale,
            });
            x += position.x_advance as f32 * scale;
            baseline -= position.y_advance as f32 * scale;
        }
        byte_offset += line.len() + 1;
    }
    Ok(TextLayout {
        glyphs,
        line_widths,
    })
}

pub fn render(params: &TextParams, bytes: &[u8], limits: &ResourceLimits) -> Result<Raster> {
    params.validate()?;
    let count = limits.check_dimensions(params.width, params.height)?;
    if bytes.len() as u64 > limits.max_input_bytes {
        return Err(PicError::new(
            ErrorCode::ResourceLimit,
            "font exceeds input byte limit",
        ));
    }
    let layout = layout(params, bytes)?;
    let font =
        FontRef::try_from_slice(bytes).map_err(|_| invalid_font("cannot read font outlines"))?;
    let scale = params.size * font.height_unscaled()
        / font
            .units_per_em()
            .ok_or_else(|| invalid_font("invalid font units per em"))?;
    if !scale.is_finite() || scale <= 0.0 {
        return Err(invalid_font("invalid font vertical metrics"));
    }
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(count)
        .map_err(|_| PicError::new(ErrorCode::ResourceLimit, "cannot allocate text box"))?;
    pixels.resize(count, [0.0; 4]);
    let color = params.color.linear();
    for placed in layout.glyphs {
        let glyph = Glyph {
            id: GlyphId(placed.id),
            scale: PxScale::from(scale),
            position: point(placed.x, placed.baseline),
        };
        if let Some(outline) = font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            // Font outlines are untrusted; admit the rasterizer's glyph scratch before drawing.
            let w = bounds.width();
            let h = bounds.height();
            if !w.is_finite()
                || !h.is_finite()
                || w < 0.0
                || h < 0.0
                || f64::from(w) * f64::from(h) * 8.0 + count as f64 * 16.0
                    > limits.max_buffer_bytes as f64
            {
                return Err(PicError::new(
                    ErrorCode::ResourceLimit,
                    "glyph raster exceeds memory budget",
                ));
            }
            outline.draw(|x, y, coverage| {
                let x = bounds.min.x as i64 + i64::from(x);
                let y = bounds.min.y as i64 + i64::from(y);
                if x >= 0 && y >= 0 && x < i64::from(params.width) && y < i64::from(params.height) {
                    let index = y as usize * params.width as usize + x as usize;
                    let mut source = color;
                    source[3] *= coverage.clamp(0.0, 1.0);
                    pixels[index] = blend(pixels[index], source, 1.0, BlendMode::Normal);
                }
            });
        }
    }
    Raster::from_linear_rgba(params.width, params.height, pixels, limits)
}
