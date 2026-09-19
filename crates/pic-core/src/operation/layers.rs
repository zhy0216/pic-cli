//! Explicit layer/selection authoring parameters, shared by CLI and project replay.
use crate::{
    ErrorCode, PicError, Result,
    composite::{BlendMode, Transform},
    document::{Selection, TargetId},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerAddParams {
    pub id: TargetId,
    pub source: String,
    pub name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerSetParams {
    pub name: Option<String>,
    pub visible: Option<bool>,
    pub opacity: Option<f64>,
    pub blend: Option<BlendMode>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReorderParams {
    /// Insert immediately below this stable ID; null moves to the top.
    pub before: Option<TargetId>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaskParams {
    pub source: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositeParams {
    pub source: String,
    pub mask: Option<String>,
    pub opacity: f64,
    pub blend: BlendMode,
    pub transform: Transform,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupParams {
    pub id: TargetId,
    pub name: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParentParams {
    pub parent: Option<TargetId>,
    pub before: Option<TargetId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipParams {
    pub base: Option<TargetId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextAddParams {
    pub id: TargetId,
    pub name: String,
    pub text: crate::text::TextParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdjustmentParams {
    pub op: String,
    pub params: serde_json::Value,
}

impl AdjustmentParams {
    pub fn operation(&self) -> Result<super::Operation> {
        if !matches!(
            self.op.as_str(),
            "adjust" | "levels" | "curves" | "grayscale" | "invert"
        ) {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "adjustment layers accept adjust, levels, curves, grayscale or invert only",
            ));
        }
        super::OperationSpec {
            op: self.op.clone(),
            op_version: super::OP_VERSION,
            target: TargetId::canvas(),
            params: self.params.clone(),
        }
        .validate()
    }

    pub(crate) fn apply(
        &self,
        raster: &crate::document::Raster,
        limits: &crate::limits::ResourceLimits,
    ) -> Result<crate::document::Raster> {
        use super::{Operation, adjustments};
        match self.operation()? {
            Operation::Adjust(p) => adjustments::adjust(raster, &p, limits),
            Operation::Levels(p) => adjustments::levels(raster, &p, limits),
            Operation::Curves(p) => adjustments::curves(raster, &p, limits),
            Operation::Grayscale => adjustments::grayscale(raster, limits),
            Operation::Invert => adjustments::invert(raster, limits),
            _ => unreachable!("validated point adjustment"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdjustmentAddParams {
    pub id: TargetId,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub adjustment: AdjustmentParams,
}

#[derive(Debug, Clone)]
pub enum LayerOperation {
    Add(LayerAddParams),
    GroupAdd(GroupParams),
    Parent(ParentParams),
    Clip(ClipParams),
    TextAdd(TextAddParams),
    TextSet(crate::text::TextParams),
    AdjustmentAdd(AdjustmentAddParams),
    AdjustmentSet(AdjustmentParams),
    Set(LayerSetParams),
    Transform(Transform),
    Reorder(ReorderParams),
    Remove,
    MaskSet(MaskParams),
    MaskRemove,
    SelectionSet(Selection),
    SelectionClear,
}

pub fn valid_id(id: &TargetId) -> Result<()> {
    if id.0.is_empty()
        || id.0.len() > 128
        || id.0 == "canvas"
        || !id
            .0
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(PicError::new(
            ErrorCode::InvalidTarget,
            "layer ID must contain 1..128 ASCII letters, digits, '_' or '-', and cannot be 'canvas'",
        ));
    }
    Ok(())
}
pub(crate) fn opacity(value: f64) -> Result<()> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(PicError::new(
            ErrorCode::InvalidArgument,
            "opacity must be finite and in [0,1]",
        ));
    }
    Ok(())
}
pub(crate) fn source(value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(PicError::new(
            ErrorCode::InvalidArgument,
            "asset source cannot be empty",
        ));
    }
    Ok(())
}
impl CompositeParams {
    pub fn validate(&self) -> Result<()> {
        source(&self.source)?;
        if let Some(mask) = &self.mask {
            source(mask)?;
        }
        opacity(self.opacity)?;
        self.transform.validate()
    }
}
impl LayerOperation {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::GroupAdd(p) => {
                valid_id(&p.id)?;
                dimensions(p.width, p.height)
            }
            Self::Parent(p) => {
                p.parent.as_ref().map_or(Ok(()), valid_id)?;
                p.before.as_ref().map_or(Ok(()), valid_id)
            }
            Self::Clip(p) => p.base.as_ref().map_or(Ok(()), valid_id),
            Self::TextAdd(p) => {
                valid_id(&p.id)?;
                p.text.validate()
            }
            Self::TextSet(p) => p.validate(),
            Self::AdjustmentAdd(p) => {
                valid_id(&p.id)?;
                dimensions(p.width, p.height)?;
                p.adjustment.operation().map(|_| ())
            }
            Self::AdjustmentSet(p) => p.operation().map(|_| ()),
            Self::Add(p) => {
                valid_id(&p.id)?;
                source(&p.source)
            }
            Self::Set(p) => p.opacity.map_or(Ok(()), opacity),
            Self::Transform(p) => p.validate(),
            Self::Reorder(p) => p.before.as_ref().map_or(Ok(()), valid_id),
            Self::MaskSet(p) => source(&p.source),
            Self::SelectionSet(p) => p.validate(),
            _ => Ok(()),
        }
    }
    pub(crate) fn specification(&self) -> (&'static str, serde_json::Value) {
        use serde_json::json;
        match self {
            Self::GroupAdd(p) => ("group_add", json!(p)),
            Self::Parent(p) => ("layer_parent", json!(p)),
            Self::Clip(p) => ("layer_clip", json!(p)),
            Self::TextAdd(p) => ("text_add", json!(p)),
            Self::TextSet(p) => ("text_set", json!(p)),
            Self::AdjustmentAdd(p) => ("adjustment_add", json!(p)),
            Self::AdjustmentSet(p) => ("adjustment_set", json!(p)),
            Self::Add(p) => ("layer_add", json!(p)),
            Self::Set(p) => ("layer_set", json!(p)),
            Self::Transform(p) => ("layer_transform", json!(p)),
            Self::Reorder(p) => ("layer_reorder", json!(p)),
            Self::Remove => ("layer_remove", json!({})),
            Self::MaskSet(p) => ("mask_set", json!(p)),
            Self::MaskRemove => ("mask_remove", json!({})),
            Self::SelectionSet(p) => ("selection_set", json!(p)),
            Self::SelectionClear => ("selection_clear", json!({})),
        }
    }
}

fn dimensions(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(PicError::new(
            ErrorCode::InvalidArgument,
            "layer bounds must be positive",
        ));
    }
    Ok(())
}
