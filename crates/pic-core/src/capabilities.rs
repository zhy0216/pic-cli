use serde::Serialize;

use crate::{
    document::PIXEL_SEMANTICS, limits::ResourceLimits, operation::OP_VERSION,
    pipeline::PIPELINE_SCHEMA_VERSION, result::RESULT_SCHEMA_VERSION,
};

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    Supported,
    Partial,
    NotImplemented,
}

#[derive(Debug, Serialize)]
pub struct Capability {
    pub id: &'static str,
    pub status: Support,
    /// `current` is implemented here, `planned` belongs to later direct-edit tasks,
    /// and `roadmap` is outside the current direct-editing scope.
    pub scope: &'static str,
    pub details: &'static str,
}

#[derive(Debug, Serialize)]
pub struct OperationCapability {
    pub op: &'static str,
    pub op_version: u32,
    pub targets: [&'static str; 1],
    pub params: serde_json::Value,
    pub semantics: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Capabilities {
    pub commands: [&'static str; 11],
    pub result_schema_version: u32,
    pub pipeline_schema_version: u32,
    pub pixel_semantics: &'static str,
    pub operations: Vec<OperationCapability>,
    pub coordinates: &'static str,
    pub encoding: serde_json::Value,
    pub capabilities: Vec<Capability>,
    pub limits: ResourceLimits,
}

pub fn capabilities() -> Capabilities {
    use Support::*;
    Capabilities {
        commands: [
            "help",
            "version",
            "capabilities",
            "info",
            "run",
            "identity",
            "crop",
            "resize",
            "rotate",
            "flip",
            "canvas",
        ],
        result_schema_version: RESULT_SCHEMA_VERSION,
        pipeline_schema_version: PIPELINE_SCHEMA_VERSION,
        pixel_semantics: PIXEL_SEMANTICS,
        operations: operation_capabilities(),
        coordinates: "Current canvas after EXIF normalization and each preceding step; top-left origin, x right, y down; centers (x+0.5,y+0.5); half-open rectangles; positive integer dimensions and resource limits apply to every intermediate canvas.",
        encoding: serde_json::json!({
            "format": {"values": ["png", "jpeg"], "default": "output extension"},
            "png_compression": {"type": "integer", "minimum": 0, "maximum": 9, "default": 6, "row_filter": "adaptive", "meaning": "0 uncompressed; 1..9 deflate level; lossless RGBA8"},
            "jpeg_quality": {"type": "integer", "minimum": 1, "maximum": 100, "default": 90},
            "jpeg_background": {"cli": "#RRGGBB or opaque #RRGGBBAA", "result": "[r,g,b,255]", "default": null, "meaning": "null rejects transparent JPEG; explicit background composites in linear sRGB at export only"},
            "format_specific_parameters": "Parameters for the other format are rejected. Export metadata is not preserved; pixels are quantized only at export."
        }),
        capabilities: vec![
            Capability {
                id: "geometry",
                status: Supported,
                scope: "current",
                details: "Crop, resize (nearest/bilinear), clockwise rotation [-360,360] (nearest/bilinear, expanded or fixed canvas), horizontal/vertical flip and canvas padding/clipping with nine anchors. JSON parameters explicit; CLI defaults returned in steps.params.",
            },
            Capability {
                id: "exif_orientation",
                status: Supported,
                scope: "current",
                details: "JPEG EXIF and PNG eXIf primary orientation 1..8 normalized before operations/info; absent orientation is 1. Classic TIFF IFD0/Exif color declarations inspected. Malformed/duplicate EXIF rejected; non-sRGB/uncalibrated EXIF color rejected.",
            },
            Capability {
                id: "info",
                status: Supported,
                scope: "current",
                details: "Full decode of accepted PNG/JPEG; normalized and stored dimensions, original EXIF orientation and color assumption.",
            },
            Capability {
                id: "identity",
                status: Supported,
                scope: "current",
                details: "Preserves in-memory samples; final codec conversion still applies.",
            },
            Capability {
                id: "pipeline",
                status: Partial,
                scope: "current",
                details: "Schema v1, ordered identity and geometry operations, explicit parameters and logical steps; unknown operations rejected.",
            },
            Capability {
                id: "png",
                status: Partial,
                scope: "current",
                details: "Static 8-bit gray/RGB/RGBA, optional sRGB/EXIF. No palette, ICC, gamma/chromaticity, HDR or animation. RGBA8 output; deflate level 0..9 (default 6), adaptive row filtering.",
            },
            Capability {
                id: "jpeg",
                status: Partial,
                scope: "current",
                details: "8-bit gray/RGB/YCbCr without ICC, EXIF orientation normalized. No CMYK/YCCK/high bit depth. RGB output, quality 1..100 (default 90); transparency requires explicit opaque JPEG background.",
            },
            Capability {
                id: "photo_editing",
                status: Partial,
                scope: "current",
                details: "Geometry supported; exposure, color adjustments, curves and filters remain planned.",
            },
            Capability {
                id: "layer_compositing",
                status: NotImplemented,
                scope: "planned",
                details: "Layers, masks, blend modes, groups, text and adjustment layers.",
            },
            Capability {
                id: "project_history_preview",
                status: NotImplemented,
                scope: "planned",
                details: "Persistent assets/ops, revisions, replay, checkpoints, undo/redo and step previews.",
            },
            Capability {
                id: "smart_editing",
                status: NotImplemented,
                scope: "roadmap",
                details: "Future only: cutout, segmentation, smart repair, generative fill and outpaint. No LLM, model, or backend integration.",
            },
            Capability {
                id: "photoshop_compatibility",
                status: NotImplemented,
                scope: "roadmap",
                details: "No PSD round-trip or Photoshop parameter/pixel compatibility claim.",
            },
        ],
        limits: ResourceLimits::default(),
    }
}

fn operation_capabilities() -> Vec<OperationCapability> {
    use serde_json::json;
    let dimension = json!({"type":"integer", "minimum":1, "maximum":u32::MAX});
    let coordinate = json!({"type":"integer", "minimum":0, "maximum":u32::MAX});
    let filter = json!({"enum":["nearest", "bilinear"], "cli_default":"bilinear"});
    let background = json!({"type":"array", "items":{"type":"integer", "minimum":0, "maximum":255}, "minItems":4, "maxItems":4, "cli_default":[0,0,0,0]});
    vec![
        OperationCapability {
            op: "identity",
            op_version: OP_VERSION,
            targets: ["canvas"],
            params: serde_json::json!({}),
            semantics: "Exact f32 samples and shared storage.",
        },
        OperationCapability {
            op: "crop",
            op_version: OP_VERSION,
            targets: ["canvas"],
            params: json!({"type":"object", "additionalProperties":false, "required":["x","y","width","height"], "properties":{"x":coordinate,"y":coordinate,"width":dimension,"height":dimension}}),
            semantics: "Rectangle must lie entirely inside the current canvas; no implicit clipping or padding. Exact pixel copy.",
        },
        OperationCapability {
            op: "resize",
            op_version: OP_VERSION,
            targets: ["canvas"],
            params: json!({"type":"object", "additionalProperties":false, "required":["filter"], "properties":{"width":{"anyOf":[dimension,{"type":"null"}]},"height":{"anyOf":[dimension,{"type":"null"}]},"filter":filter}}),
            semantics: "At least one dimension required. Both set exact dimensions; one infers aspect ratio, nearest integer (half up), minimum 1. Nearest copies center-mapped pixels. Bilinear uses fast_image_resize 5.5.0 triangular convolution, widened for reduction, normalized at edges, premultiplied linear sRGB. Zero-alpha filtered RGB becomes zero; no RGB gamut clipping. Same-size resize is exact identity.",
        },
        OperationCapability {
            op: "rotate",
            op_version: OP_VERSION,
            targets: ["canvas"],
            params: json!({"type":"object", "additionalProperties":false, "required":["degrees","expand","filter","background"], "properties":{"degrees":{"type":"number","minimum":-360,"maximum":360},"expand":{"type":"boolean","cli_default":true},"filter":filter,"background":background}}),
            semantics: "Clockwise about canvas center. Expanded size ceil(|w*cos|+|h*sin|) by ceil(|w*sin|+|h*cos|), centers aligned. Exact quarter turns when expanded or square, exact 0/180 always. Fixed non-square quarter turns sample. Inverse center sampling; outside taps use background. Bilinear blends premultiplied linear sRGB; zero-alpha filtered RGB is zero. Nearest ties right/bottom.",
        },
        OperationCapability {
            op: "flip",
            op_version: OP_VERSION,
            targets: ["canvas"],
            params: json!({"type":"object", "additionalProperties":false, "required":["axis"], "properties":{"axis":{"enum":["horizontal","vertical"]}}}),
            semantics: "Horizontal reverses x; vertical reverses y. Dimensions and f32 samples preserved.",
        },
        OperationCapability {
            op: "canvas",
            op_version: OP_VERSION,
            targets: ["canvas"],
            params: json!({"type":"object", "additionalProperties":false, "required":["width","height","anchor","background"], "properties":{"width":dimension,"height":dimension,"anchor":{"enum":["top_left","top","top_right","left","center","right","bottom_left","bottom","bottom_right"],"cli_default":"center"},"background":background}}),
            semantics: "Padding/clipping without scaling. Start/center/end offsets are 0/floor((new-old)/2)/(new-old). Background only fills uncovered canvas; copied source alpha and hidden RGB stay intact.",
        },
    ]
}
