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
}

#[derive(Debug, Serialize)]
pub struct Capabilities {
    pub commands: [&'static str; 6],
    pub result_schema_version: u32,
    pub pipeline_schema_version: u32,
    pub pixel_semantics: &'static str,
    pub operations: Vec<OperationCapability>,
    pub capabilities: Vec<Capability>,
    pub limits: ResourceLimits,
}

pub fn capabilities() -> Capabilities {
    use Support::*;
    Capabilities {
        commands: ["help", "version", "capabilities", "info", "run", "identity"],
        result_schema_version: RESULT_SCHEMA_VERSION,
        pipeline_schema_version: PIPELINE_SCHEMA_VERSION,
        pixel_semantics: PIXEL_SEMANTICS,
        operations: vec![OperationCapability {
            op: "identity",
            op_version: OP_VERSION,
            targets: ["canvas"],
            params: serde_json::json!({}),
        }],
        capabilities: vec![
            Capability {
                id: "info",
                status: Supported,
                scope: "current",
                details: "Full decode and inspection of accepted PNG/JPEG inputs.",
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
                details: "Schema v1, empty or ordered identity operations only; unknown operations rejected.",
            },
            Capability {
                id: "png",
                status: Partial,
                scope: "current",
                details: "Static 8-bit gray/RGB/RGBA, optional sRGB tag. No palette, ICC, gamma/chromaticity, HDR, or EXIF. RGBA8 output.",
            },
            Capability {
                id: "jpeg",
                status: Partial,
                scope: "current",
                details: "8-bit gray/RGB/YCbCr without ICC or EXIF. Opaque RGB output, quality 1..100 (default 90); alpha rejected.",
            },
            Capability {
                id: "photo_editing",
                status: NotImplemented,
                scope: "planned",
                details: "Geometry, exposure, color adjustments, curves and filters.",
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
