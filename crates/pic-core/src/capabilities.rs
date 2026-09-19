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
    pub commands: [&'static str; 25],
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
            "adjust",
            "levels",
            "curves",
            "grayscale",
            "invert",
            "blur",
            "sharpen",
            "project create",
            "project apply",
            "project inspect",
            "project export",
            "project preview",
            "project undo",
            "project redo",
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
                details: "Schema v1, ordered identity, geometry, adjustment and filter operations, explicit parameters and logical steps; unknown operations rejected. One decode and final encode; no intermediate quantization or step fusion.",
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
                details: "Geometry, exposure EV, brightness, contrast, saturation, levels, piecewise linear curves, grayscale, invert, Gaussian blur and unsharp mask supported in linear sRGB RGBA32F. No Photoshop parameter or pixel equivalence claim.",
            },
            Capability {
                id: "adjustments_filters",
                status: Supported,
                scope: "current",
                details: "Scalar v1 semantics; finite parameters only. RGB is not clipped between steps; non-finite f32 results fail. Color operations preserve alpha, blur filters premultiplied color/alpha with clamped edges, sharpen preserves original alpha. JSON requires every parameter; CLI defaults returned in steps.params.",
            },
            Capability {
                id: "layer_compositing",
                status: NotImplemented,
                scope: "planned",
                details: "Layers, masks, blend modes, groups, text and adjustment layers.",
            },
            Capability {
                id: "project_history_preview",
                status: Partial,
                scope: "current",
                details: "Self-contained .pic assets and immutable normalized ops; SHA-256 integrity/dedup, atomic manifest publication, cross-process writer lock and required expected_revision. Group undo/redo, any committed step inspect/export/full-resolution preview, continuation from old steps with retained read-only history. Full RGBA32F replay via the same pipeline. Checkpoints, scaled/region previews and templates remain planned. Linux validated.",
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
    let number = |min, max, default| json!({"type":"number", "minimum":min, "maximum":max, "cli_default":default});
    let channel = json!({"enum":["rgb","red","green","blue"], "cli_default":"rgb"});
    let sigma = number(0.0, 100.0, 1.0);
    let empty = json!({"type":"object", "additionalProperties":false, "properties":{}});
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
        OperationCapability {
            op: "adjust",
            op_version: OP_VERSION,
            targets: ["canvas"],
            params: json!({"type":"object", "additionalProperties":false, "required":["exposure","brightness","contrast","saturation"], "properties":{"exposure":number(-20.0,20.0,0.0),"brightness":number(-1.0,1.0,0.0),"contrast":number(0.0,10.0,1.0),"saturation":number(0.0,10.0,1.0)}}),
            semantics: "All RGB, including hidden color, in linear sRGB: C*=2^exposure, C+=brightness, C=(C-0.5)*contrast+0.5, then C=Y+saturation*(C-Y), Y=0.2126R+0.7152G+0.0722B. Neutral controls skipped; all neutral shares exact samples. Scalar f64 within this operation, f32 result, no clipping; alpha bits unchanged.",
        },
        OperationCapability {
            op: "levels",
            op_version: OP_VERSION,
            targets: ["canvas"],
            params: json!({"type":"object", "additionalProperties":false, "required":["channel","input_black","input_white","gamma","output_black","output_white"], "properties":{"channel":channel,"input_black":number(0.0,1.0,0.0),"input_white":number(0.0,1.0,1.0),"gamma":number(0.1,10.0,1.0),"output_black":number(0.0,1.0,0.0),"output_white":number(0.0,1.0,1.0)}}),
            semantics: "Linear RGB selected channels, including hidden color. input_black<input_white, output_black<=output_white. t=(C-input_black)/(input_white-input_black); C'=output_black+(output_white-output_black)*sign(t)*abs(t)^(1/gamma). Signed extension preserves negative/HDR inputs, no clipping. Equal output endpoints yield a constant. Gamma 1 and matching input/output endpoints are exact identity. Scalar f64 then f32; other channels and alpha unchanged.",
        },
        OperationCapability {
            op: "curves",
            op_version: OP_VERSION,
            targets: ["canvas"],
            params: json!({"type":"object", "additionalProperties":false, "required":["channel","points"], "properties":{"channel":channel,"points":{"type":"array","minItems":2,"maxItems":256,"items":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number","minimum":0,"maximum":1}},"cli_default":[[0,0],[1,1]]}}}),
            semantics: "Linear RGB selected channels, including hidden color. Points [x,y], strictly increasing x from 0 to 1; y need not be monotonic. Piecewise linear interpolation, exact control values, first/last segment extrapolation outside [0,1]. All x=y gives exact identity. Scalar f64 then f32; no clipping, other channels and alpha unchanged.",
        },
        OperationCapability {
            op: "grayscale",
            op_version: OP_VERSION,
            targets: ["canvas"],
            params: empty.clone(),
            semantics: "Linear RGB becomes Rec.709 Y=0.2126R+0.7152G+0.0722B, evaluated as G+0.2126*(R-G)+0.0722*(B-G). Hidden RGB is transformed; alpha bits unchanged. Scalar f64 then f32, no clipping.",
        },
        OperationCapability {
            op: "invert",
            op_version: OP_VERSION,
            targets: ["canvas"],
            params: empty,
            semantics: "C'=1-C on all linear RGB including hidden color; scalar f64 then f32, no clipping. Alpha bits unchanged. This is not byte-wise sRGB inversion.",
        },
        OperationCapability {
            op: "blur",
            op_version: OP_VERSION,
            targets: ["canvas"],
            params: json!({"type":"object", "additionalProperties":false, "required":["sigma"], "properties":{"sigma":sigma}}),
            semantics: "Gaussian standard deviation in pixels; 0 exact identity. Radius ceil(3*sigma), weights exp(-0.5*(offset/sigma)^2) normalized in f64. Horizontal then vertical scalar passes in premultiplied linear RGBA64F; outside taps clamp to nearest edge. Final unpremultiply and f32 conversion; zero stored alpha yields zero RGB. Dimensions unchanged; no RGB clipping. Buffers: 64 bytes/pixel plus 8 bytes/tap.",
        },
        OperationCapability {
            op: "sharpen",
            op_version: OP_VERSION,
            targets: ["canvas"],
            params: json!({"type":"object", "additionalProperties":false, "required":["sigma","amount"], "properties":{"sigma":sigma,"amount":number(0.0,10.0,1.0)}}),
            semantics: "Unsharp mask: B is the same blur v1 f32 result; visible RGB C'=C+amount*(C-B). Scalar f64 then f32, overshoot retained. Original alpha bits and zero-alpha hidden RGB preserved. sigma=0 or amount=0 exact identity. Same clamped-edge Gaussian and peak buffer budget as blur.",
        },
    ]
}
