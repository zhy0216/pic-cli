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

#[derive(Debug, Clone)]
pub enum LayerOperation {
    Add(LayerAddParams),
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
