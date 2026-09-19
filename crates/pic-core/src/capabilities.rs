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
    pub targets: Vec<&'static str>,
    pub params: serde_json::Value,
    pub semantics: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Capabilities {
    pub commands: Vec<&'static str>,
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
        commands: vec![
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
            "project checkpoint",
            "project cache-clear",
            "project revise",
            "project template-export",
            "project template-run",
            "composite",
            "project layer add",
            "project layer set",
            "project layer transform",
            "project layer reorder",
            "project layer remove",
            "project layer edit",
            "project layer parent",
            "project layer clip",
            "project group add",
            "project text add",
            "project text set",
            "project adjustment add",
            "project adjustment set",
            "project mask set",
            "project mask remove",
            "project selection set",
            "project selection clear",
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
                details: "Schema v1, ordered pixel, composite, layer, mask and selection operations with explicit targets/parameters and logical steps; unknown operations rejected. Input/resources decode to RGBA32F; final encode only, no intermediate quantization or step fusion.",
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
                status: Partial,
                scope: "current",
                details: "Stable independent layers, sibling order by ID, visibility/opacity, non-destructive transforms, normal/multiply/screen/overlay in linear sRGB, encoded grayscale coverage masks and explicit-space rectangular selections. Isolated bounded groups (32 ancestor levels), lower-sibling clipping chains, editable text and point adjustment layers use the same Document/ops/render core. Missing dependencies, cycles, cross-scope/forward clips and removal of referenced targets are rejected.",
            },
            Capability {
                id: "text_layout",
                status: Partial,
                scope: "current",
                details: "Explicit embedded static single-face TrueType outline font; no lookup/fallback. Rustybuzz 0.20.1 OpenType shaping and ab_glyph 0.2.32 antialiased coverage. Horizontal LTR Latin/Greek/Cyrillic (one alphabet per LF-delimited line), Common/Inherited; glyph coverage checked. LF only, left/center/right advance alignment, fixed box clips overflow, no auto wrap. No CJK, bidi, complex scripts, variable/color/bitmap fonts or collections. Missing glyph/font and unsupported text fail explicitly.",
            },
            Capability {
                id: "adjustment_layers",
                status: Partial,
                scope: "current",
                details: "Non-destructive adjust/levels/curves/grayscale/invert, v1 point formulas, lower accumulated sibling prefix only, bounded in parent scope. Interpolate RGB by transformed coverage * clip alpha * opacity, preserve backdrop alpha. Normal blend only; no spatial filter adjustment layers or pass-through groups.",
            },
            Capability {
                id: "project_history_preview",
                status: Partial,
                scope: "current",
                details: "Self-contained .pic assets and immutable ops; atomic manifest, cross-process writer lock and expected_revision. Group undo/redo, any committed revision, old-step parameter revision and prefix reuse reports. Exact RGBA32F checkpoints and separate region/scaled preview caches with disk/memory budgets, integrity fallback, bidirectional canvas coordinates. Templates require explicit new input, canvas target and all step parameters, creating new project-local history; asset/content-dependent operations rejected until rebinding support exists. Complete editable layer/mask/transform/selection checkpoints; canvas/layer/mask previews and affine mappings. Legacy canvas history retains exact pixel semantics. Layered canvas supports identity, non-destructive crop and transparent canvas; other edits require layer IDs. Linux validated.",
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
    let mut operations = vec![
        OperationCapability {
            op: "identity",
            op_version: OP_VERSION,
            targets: vec!["canvas", "layer_id"],
            params: serde_json::json!({}),
            semantics: "Exact f32 samples and shared storage.",
        },
        OperationCapability {
            op: "crop",
            op_version: OP_VERSION,
            targets: vec!["canvas", "layer_id"],
            params: json!({"type":"object", "additionalProperties":false, "required":["x","y","width","height"], "properties":{"x":coordinate,"y":coordinate,"width":dimension,"height":dimension}}),
            semantics: "Rectangle must lie entirely inside the current canvas; no implicit clipping or padding. Exact pixel copy.",
        },
        OperationCapability {
            op: "resize",
            op_version: OP_VERSION,
            targets: vec!["canvas", "layer_id"],
            params: json!({"type":"object", "additionalProperties":false, "required":["filter"], "properties":{"width":{"anyOf":[dimension,{"type":"null"}]},"height":{"anyOf":[dimension,{"type":"null"}]},"filter":filter}}),
            semantics: "At least one dimension required. Both set exact dimensions; one infers aspect ratio, nearest integer (half up), minimum 1. Nearest copies center-mapped pixels. Bilinear uses fast_image_resize 5.5.0 triangular convolution, widened for reduction, normalized at edges, premultiplied linear sRGB. Zero-alpha filtered RGB becomes zero; no RGB gamut clipping. Same-size resize is exact identity.",
        },
        OperationCapability {
            op: "rotate",
            op_version: OP_VERSION,
            targets: vec!["canvas", "layer_id"],
            params: json!({"type":"object", "additionalProperties":false, "required":["degrees","expand","filter","background"], "properties":{"degrees":{"type":"number","minimum":-360,"maximum":360},"expand":{"type":"boolean","cli_default":true},"filter":filter,"background":background}}),
            semantics: "Clockwise about canvas center. Expanded size ceil(|w*cos|+|h*sin|) by ceil(|w*sin|+|h*cos|), centers aligned. Exact quarter turns when expanded or square, exact 0/180 always. Fixed non-square quarter turns sample. Inverse center sampling; outside taps use background. Bilinear blends premultiplied linear sRGB; zero-alpha filtered RGB is zero. Nearest ties right/bottom.",
        },
        OperationCapability {
            op: "flip",
            op_version: OP_VERSION,
            targets: vec!["canvas", "layer_id"],
            params: json!({"type":"object", "additionalProperties":false, "required":["axis"], "properties":{"axis":{"enum":["horizontal","vertical"]}}}),
            semantics: "Horizontal reverses x; vertical reverses y. Dimensions and f32 samples preserved.",
        },
        OperationCapability {
            op: "canvas",
            op_version: OP_VERSION,
            targets: vec!["canvas", "layer_id"],
            params: json!({"type":"object", "additionalProperties":false, "required":["width","height","anchor","background"], "properties":{"width":dimension,"height":dimension,"anchor":{"enum":["top_left","top","top_right","left","center","right","bottom_left","bottom","bottom_right"],"cli_default":"center"},"background":background}}),
            semantics: "Padding/clipping without scaling. Start/center/end offsets are 0/floor((new-old)/2)/(new-old). Background only fills uncovered canvas; copied source alpha and hidden RGB stay intact.",
        },
        OperationCapability {
            op: "adjust",
            op_version: OP_VERSION,
            targets: vec!["canvas", "layer_id"],
            params: json!({"type":"object", "additionalProperties":false, "required":["exposure","brightness","contrast","saturation"], "properties":{"exposure":number(-20.0,20.0,0.0),"brightness":number(-1.0,1.0,0.0),"contrast":number(0.0,10.0,1.0),"saturation":number(0.0,10.0,1.0)}}),
            semantics: "All RGB, including hidden color, in linear sRGB: C*=2^exposure, C+=brightness, C=(C-0.5)*contrast+0.5, then C=Y+saturation*(C-Y), Y=0.2126R+0.7152G+0.0722B. Neutral controls skipped; all neutral shares exact samples. Scalar f64 within this operation, f32 result, no clipping; alpha bits unchanged.",
        },
        OperationCapability {
            op: "levels",
            op_version: OP_VERSION,
            targets: vec!["canvas", "layer_id"],
            params: json!({"type":"object", "additionalProperties":false, "required":["channel","input_black","input_white","gamma","output_black","output_white"], "properties":{"channel":channel,"input_black":number(0.0,1.0,0.0),"input_white":number(0.0,1.0,1.0),"gamma":number(0.1,10.0,1.0),"output_black":number(0.0,1.0,0.0),"output_white":number(0.0,1.0,1.0)}}),
            semantics: "Linear RGB selected channels, including hidden color. input_black<input_white, output_black<=output_white. t=(C-input_black)/(input_white-input_black); C'=output_black+(output_white-output_black)*sign(t)*abs(t)^(1/gamma). Signed extension preserves negative/HDR inputs, no clipping. Equal output endpoints yield a constant. Gamma 1 and matching input/output endpoints are exact identity. Scalar f64 then f32; other channels and alpha unchanged.",
        },
        OperationCapability {
            op: "curves",
            op_version: OP_VERSION,
            targets: vec!["canvas", "layer_id"],
            params: json!({"type":"object", "additionalProperties":false, "required":["channel","points"], "properties":{"channel":channel,"points":{"type":"array","minItems":2,"maxItems":256,"items":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number","minimum":0,"maximum":1}},"cli_default":[[0,0],[1,1]]}}}),
            semantics: "Linear RGB selected channels, including hidden color. Points [x,y], strictly increasing x from 0 to 1; y need not be monotonic. Piecewise linear interpolation, exact control values, first/last segment extrapolation outside [0,1]. All x=y gives exact identity. Scalar f64 then f32; no clipping, other channels and alpha unchanged.",
        },
        OperationCapability {
            op: "grayscale",
            op_version: OP_VERSION,
            targets: vec!["canvas", "layer_id"],
            params: empty.clone(),
            semantics: "Linear RGB becomes Rec.709 Y=0.2126R+0.7152G+0.0722B, evaluated as G+0.2126*(R-G)+0.0722*(B-G). Hidden RGB is transformed; alpha bits unchanged. Scalar f64 then f32, no clipping.",
        },
        OperationCapability {
            op: "invert",
            op_version: OP_VERSION,
            targets: vec!["canvas", "layer_id"],
            params: empty,
            semantics: "C'=1-C on all linear RGB including hidden color; scalar f64 then f32, no clipping. Alpha bits unchanged. This is not byte-wise sRGB inversion.",
        },
        OperationCapability {
            op: "blur",
            op_version: OP_VERSION,
            targets: vec!["canvas", "layer_id"],
            params: json!({"type":"object", "additionalProperties":false, "required":["sigma"], "properties":{"sigma":sigma}}),
            semantics: "Gaussian standard deviation in pixels; 0 exact identity. Radius ceil(3*sigma), weights exp(-0.5*(offset/sigma)^2) normalized in f64. Horizontal then vertical scalar passes in premultiplied linear RGBA64F; outside taps clamp to nearest edge. Final unpremultiply and f32 conversion; zero stored alpha yields zero RGB. Dimensions unchanged; no RGB clipping. Buffers: 64 bytes/pixel plus 8 bytes/tap.",
        },
        OperationCapability {
            op: "sharpen",
            op_version: OP_VERSION,
            targets: vec!["canvas", "layer_id"],
            params: json!({"type":"object", "additionalProperties":false, "required":["sigma","amount"], "properties":{"sigma":sigma,"amount":number(0.0,10.0,1.0)}}),
            semantics: "Unsharp mask: B is the same blur v1 f32 result; visible RGB C'=C+amount*(C-B). Scalar f64 then f32, overshoot retained. Original alpha bits and zero-alpha hidden RGB preserved. sigma=0 or amount=0 exact identity. Same clamped-edge Gaussian and peak buffer budget as blur.",
        },
    ];
    operations.extend(advanced_layer_capabilities(&operations));
    operations.extend(layer_capabilities());
    operations
}

fn layer_capabilities() -> Vec<OperationCapability> {
    use serde_json::json;
    let string = json!({"type":"string","minLength":1});
    let transform = json!({"type":"object","additionalProperties":false,"required":["x","y","scale_x","scale_y","degrees","flip_x","flip_y","filter"],"properties":{
        "x":{"type":"number","minimum":-1e9,"maximum":1e9},"y":{"type":"number","minimum":-1e9,"maximum":1e9},
        "scale_x":{"type":"number","minimum":1e-6,"maximum":1e6},"scale_y":{"type":"number","minimum":1e-6,"maximum":1e6},
        "degrees":{"type":"number","minimum":-360,"maximum":360},"flip_x":{"type":"boolean"},"flip_y":{"type":"boolean"},"filter":{"enum":["nearest","bilinear"]}}});
    let blend = json!({"enum":["normal","multiply","screen","overlay"]});
    let opacity = json!({"type":"number","minimum":0,"maximum":1});
    let rectangle = json!({"type":"object","additionalProperties":false,"required":["x","y","width","height"],"properties":{"x":{"type":"number"},"y":{"type":"number"},"width":{"type":"number","exclusiveMinimum":0},"height":{"type":"number","exclusiveMinimum":0}}});
    let object = |required: Vec<&str>, properties: serde_json::Value| json!({"type":"object","additionalProperties":false,"required":required,"properties":properties});
    let nullable = |value: serde_json::Value| json!({"anyOf":[value,{"type":"null"}]});
    vec![
        OperationCapability {
            op: "composite",
            op_version: OP_VERSION,
            targets: vec!["canvas", "layer_id"],
            params: object(
                vec!["source", "opacity", "blend", "transform"],
                json!({"source":string,"mask":nullable(string.clone()),"opacity":opacity,"blend":blend,"transform":transform}),
            ),
            semantics: "External source over local pixels using the layer renderer; mask same local dimensions. Legacy canvas or explicit layer target only. Source-over alpha with separable linear RGB blend; mask before premultiplied filtering. No intermediate quantization.",
        },
        OperationCapability {
            op: "layer_add",
            op_version: OP_VERSION,
            targets: vec!["canvas"],
            params: object(
                vec!["id", "source", "name"],
                json!({"id":string,"source":string,"name":{"type":"string"}}),
            ),
            semantics: "Append at top; ID is caller-defined ASCII [A-Za-z0-9_-], 1..128, never canvas, never reused on this ancestry. Base layer ID is base. Source path relative to pipeline; projects persist content-addressed bytes.",
        },
        OperationCapability {
            op: "layer_set",
            op_version: OP_VERSION,
            targets: vec!["layer_id"],
            params: object(
                vec![],
                json!({"name":nullable(json!({"type":"string"})),"visible":nullable(json!({"type":"boolean"})),"opacity":nullable(opacity),"blend":nullable(blend)}),
            ),
            semantics: "Only non-null fields change. Name is display-only. Order is bottom-to-top; hidden layers contribute nothing. Multiply B=b*s; screen B=b+s-b*s; overlay B=2bs when b<=0.5, otherwise 1-2(1-b)(1-s). RGB unclipped.",
        },
        OperationCapability {
            op: "layer_transform",
            op_version: OP_VERSION,
            targets: vec!["layer_id"],
            params: transform,
            semantics: "Replace full transform without changing local pixels/mask. Flip within local bounds, scale, rotate clockwise about local origin, translate x/y. Nearest or four-tap bilinear premultiplied linear sampling with transparent outside; no downsample antialias widening.",
        },
        OperationCapability {
            op: "layer_reorder",
            op_version: OP_VERSION,
            targets: vec!["layer_id"],
            params: object(vec![], json!({"before":nullable(string.clone())})),
            semantics: "Insert below stable sibling before ID; null moves to top of current parent scope. Self/absent/cross-scope references and broken clip ordering fail.",
        },
        OperationCapability {
            op: "layer_remove",
            op_version: OP_VERSION,
            targets: vec!["layer_id"],
            params: object(vec![], json!({})),
            semantics: "Remove layer and attached mask; reject groups with children and bases with clipping dependents. Clear a selection whose space references it. Undo restores all. ID remains reserved on this ancestry.",
        },
        OperationCapability {
            op: "mask_set",
            op_version: OP_VERSION,
            targets: vec!["layer_id"],
            params: object(vec!["source"], json!({"source":string})),
            semantics: "Same-size 8-bit grayscale PNG/JPEG (RGB requires equal channels), EXIF normalized. Coverage=(encoded_gray/255)*(alpha/255), not linear luminance. Attached mask moves with layer; persistent ID mask:<layer_id>.",
        },
        OperationCapability {
            op: "mask_remove",
            op_version: OP_VERSION,
            targets: vec!["layer_id"],
            params: object(vec![], json!({})),
            semantics: "Remove coverage restriction, retaining layer pixels and transform.",
        },
        OperationCapability {
            op: "selection_set",
            op_version: OP_VERSION,
            targets: vec!["canvas"],
            params: object(
                vec!["space", "regions"],
                json!({"space":string,"regions":{"type":"array","maxItems":1024,"items":rectangle}}),
            ),
            semantics: "Union of in-bounds half-open rectangles in canvas or stable layer space. Pixel centers map via true inverse affine transform; empty selects nothing. Restricts subsequent layer color/filter writes, not rendering or layer property edits. Geometry with selection/mask is rejected; filter reads whole local image.",
        },
        OperationCapability {
            op: "selection_clear",
            op_version: OP_VERSION,
            targets: vec!["canvas"],
            params: object(vec![], json!({})),
            semantics: "Remove selection; subsequent local pixel edits affect the entire layer.",
        },
    ]
}

fn advanced_layer_capabilities(
    point_operations: &[OperationCapability],
) -> Vec<OperationCapability> {
    use serde_json::json;
    let string = json!({"type":"string","minLength":1});
    let dimension = json!({"type":"integer","minimum":1,"maximum":u32::MAX});
    let object = |required: Vec<&str>, properties: serde_json::Value| json!({"type":"object","additionalProperties":false,"required":required,"properties":properties});
    let nullable = |value: serde_json::Value| json!({"anyOf":[value,{"type":"null"}]});
    let text = object(
        vec![
            "text",
            "font",
            "size",
            "line_height",
            "width",
            "height",
            "align",
            "color",
        ],
        json!({
            "text":{"type":"string","description":"<=65536 UTF-8 bytes, LF-delimited LTR lines; one of Latin/Greek/Cyrillic plus Common/Inherited per line"},
            "font":string,"size":{"type":"number","minimum":1,"maximum":512},
            "line_height":{"type":"number","minimum":1,"maximum":4096},"width":dimension,"height":dimension,
            "align":{"enum":["left","center","right"]},"color":{"type":"array","minItems":4,"maxItems":4,"items":{"type":"integer","minimum":0,"maximum":255}}
        }),
    );
    let adjustment = json!({"oneOf":point_operations.iter().filter(|op| matches!(op.op,"adjust"|"levels"|"curves"|"grayscale"|"invert")).map(|op|
        object(vec!["op","params"],json!({"op":{"const":op.op},"params":op.params}))).collect::<Vec<_>>()});
    vec![
        OperationCapability {
            op: "group_add",
            op_version: OP_VERSION,
            targets: vec!["canvas"],
            params: object(
                vec!["id", "name", "width", "height"],
                json!({"id":string,"name":{"type":"string"},"width":dimension,"height":dimension}),
            ),
            semantics: "Append empty isolated group at root. Explicit fixed local bounds clip children. Children composite bottom-to-top, then group mask/transform/clip/opacity/blend apply once. No pass-through. Use layer_parent to populate; no implicit coordinate conversion.",
        },
        OperationCapability {
            op: "layer_parent",
            op_version: OP_VERSION,
            targets: vec!["layer_id"],
            params: object(
                vec![],
                json!({"parent":nullable(string.clone()),"before":nullable(string.clone())}),
            ),
            semantics: "Move into named group or root (null), below destination sibling before or at top (null), preserving local transform. Descendants stay attached. Reject missing/non-group parents, cycles, depth>32, cross-scope before and broken clip dependencies.",
        },
        OperationCapability {
            op: "layer_clip",
            op_version: OP_VERSION,
            targets: vec!["layer_id"],
            params: object(vec![], json!({"base":nullable(string.clone())})),
            semantics: "Null detaches. Base must be a lower sibling raster/text/group. Source alpha *= base effective alpha after its mask/transform/opacity/visibility/clip. Chains multiply coverage; bases remain independently visible. Adjustment cannot be a base. No alpha borrowed from accumulated backdrop.",
        },
        OperationCapability {
            op: "text_add",
            op_version: OP_VERSION,
            targets: vec!["canvas"],
            params: object(
                vec!["id", "name", "text"],
                json!({"id":string,"name":{"type":"string"},"text":text}),
            ),
            semantics: "Append editable shaped text at root. Bind exact font bytes as project asset. size is px/em; first baseline is font ascender*size/upem, subsequent baselines add line_height. Align by shaped advances, clip fixed box, LF only. Default OpenType ligatures/kerning/marks; no silent font fallback.",
        },
        OperationCapability {
            op: "text_set",
            op_version: OP_VERSION,
            targets: vec!["text_layer_id"],
            params: text,
            semantics: "Replace all editable text parameters and reshape from explicit font binding; preserve transform/parent/clip/properties. Existing mask must still match new box. Pixel edits are rejected for text layers.",
        },
        OperationCapability {
            op: "adjustment_add",
            op_version: OP_VERSION,
            targets: vec!["canvas"],
            params: object(
                vec!["id", "name", "width", "height", "adjustment"],
                json!({"id":string,"name":{"type":"string"},"width":dimension,"height":dimension,"adjustment":adjustment}),
            ),
            semantics: "Append non-destructive adjustment over lower accumulated siblings only. Explicit local bounds, masks/transforms/clips/opacity weight RGB interpolation. Backdrop alpha unchanged. Group scopes isolated; normal blend required. Preview of adjustment shows adjusted scope prefix.",
        },
        OperationCapability {
            op: "adjustment_set",
            op_version: OP_VERSION,
            targets: vec!["adjustment_layer_id"],
            params: adjustment,
            semantics: "Replace point-operation parameters, retain layer bounds/properties/transform/mask/clip; lower raster pixels remain editable and unchanged.",
        },
    ]
}
