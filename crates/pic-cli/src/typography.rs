//! Thin authoring of group, text and adjustment ops through the same project commit path.
use crate::{Data, layers::EditArgs};
use clap::{Args, Subcommand};
use pic_core::{
    ErrorCode, PicError, Result,
    document::TargetId,
    limits::ResourceLimits,
    operation::{
        geometry::RgbaColor,
        layers::{
            AdjustmentAddParams, AdjustmentParams, GroupParams, LayerOperation, TextAddParams,
        },
    },
    result::Diagnostics,
    text::{TextAlign, TextParams},
};

#[derive(Args)]
pub(super) struct BoundsArgs {
    #[arg(long)]
    width: u32,
    #[arg(long)]
    height: u32,
}

#[derive(Subcommand)]
pub(super) enum GroupCommand {
    /// Create a transparent isolated group with explicit local clipping bounds
    Add {
        #[command(flatten)]
        edit: EditArgs,
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "Group")]
        name: String,
        #[command(flatten)]
        bounds: BoundsArgs,
    },
}
impl GroupCommand {
    pub fn execute(self, limits: &ResourceLimits, diagnostics: &mut Diagnostics) -> Result<Data> {
        let Self::Add {
            edit,
            id,
            name,
            bounds,
        } = self;
        edit.layer(
            "canvas".into(),
            LayerOperation::GroupAdd(GroupParams {
                id: TargetId(id),
                name,
                width: bounds.width,
                height: bounds.height,
            }),
            limits,
            diagnostics,
        )
    }
}

#[derive(Args)]
pub(super) struct TextArgs {
    /// UTF-8 content; literal LF breaks lines, box overflow clips, no automatic wrapping
    #[arg(long)]
    text: String,
    /// Static TrueType file or existing asset:SHA256 binding; never a system font name
    #[arg(long)]
    font: String,
    /// Pixels per em (1..512)
    #[arg(long, default_value_t = 24.0)]
    size: f32,
    /// Baseline distance in pixels (defaults to 1.2 * size)
    #[arg(long)]
    line_height: Option<f32>,
    #[command(flatten)]
    bounds: BoundsArgs,
    #[arg(long, default_value = "left")]
    align: TextAlign,
    #[arg(long, default_value = "#000000ff")]
    color: RgbaColor,
}
impl TextArgs {
    fn parameters(self) -> TextParams {
        TextParams {
            text: self.text,
            font: self.font,
            size: self.size,
            line_height: self.line_height.unwrap_or(self.size * 1.2),
            width: self.bounds.width,
            height: self.bounds.height,
            align: self.align,
            color: self.color,
        }
    }
}

#[derive(Subcommand)]
pub(super) enum TextCommand {
    /// Add editable LTR Latin/Greek/Cyrillic text with Common/Inherited characters
    Add {
        #[command(flatten)]
        edit: EditArgs,
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "Text")]
        name: String,
        #[command(flatten)]
        text: TextArgs,
    },
    /// Replace all text parameters; reuse the font asset binding returned by inspect
    Set {
        #[command(flatten)]
        edit: EditArgs,
        #[arg(long)]
        target: String,
        #[command(flatten)]
        text: TextArgs,
    },
}
impl TextCommand {
    pub fn execute(self, limits: &ResourceLimits, diagnostics: &mut Diagnostics) -> Result<Data> {
        match self {
            Self::Add {
                edit,
                id,
                name,
                text,
            } => edit.layer(
                "canvas".into(),
                LayerOperation::TextAdd(TextAddParams {
                    id: TargetId(id),
                    name,
                    text: text.parameters(),
                }),
                limits,
                diagnostics,
            ),
            Self::Set { edit, target, text } => edit.layer(
                target,
                LayerOperation::TextSet(text.parameters()),
                limits,
                diagnostics,
            ),
        }
    }
}

#[derive(Args)]
pub(super) struct AdjustmentArgs {
    /// adjust, levels, curves, grayscale or invert (same v1 pixel formulas)
    #[arg(long)]
    op: String,
    /// Complete point-operation parameter object
    #[arg(long, default_value = "{}")]
    params: String,
}
impl AdjustmentArgs {
    fn parameters(self) -> Result<AdjustmentParams> {
        Ok(AdjustmentParams {
            op: self.op,
            params: serde_json::from_str(&self.params)
                .map_err(|e| PicError::new(ErrorCode::InvalidJson, e.to_string()))?,
        })
    }
}
#[derive(Subcommand)]
pub(super) enum AdjustmentCommand {
    /// Add above existing layers; affects only the lower prefix inside its local bounds
    Add {
        #[command(flatten)]
        edit: EditArgs,
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "Adjustment")]
        name: String,
        #[command(flatten)]
        bounds: BoundsArgs,
        #[command(flatten)]
        adjustment: AdjustmentArgs,
    },
    /// Replace the non-destructive point operation without modifying lower-layer pixels
    Set {
        #[command(flatten)]
        edit: EditArgs,
        #[arg(long)]
        target: String,
        #[command(flatten)]
        adjustment: AdjustmentArgs,
    },
}
impl AdjustmentCommand {
    pub fn execute(self, limits: &ResourceLimits, diagnostics: &mut Diagnostics) -> Result<Data> {
        match self {
            Self::Add {
                edit,
                id,
                name,
                bounds,
                adjustment,
            } => edit.layer(
                "canvas".into(),
                LayerOperation::AdjustmentAdd(AdjustmentAddParams {
                    id: TargetId(id),
                    name,
                    width: bounds.width,
                    height: bounds.height,
                    adjustment: adjustment.parameters()?,
                }),
                limits,
                diagnostics,
            ),
            Self::Set {
                edit,
                target,
                adjustment,
            } => edit.layer(
                target,
                LayerOperation::AdjustmentSet(adjustment.parameters()?),
                limits,
                diagnostics,
            ),
        }
    }
}
