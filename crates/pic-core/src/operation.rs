use serde::{Deserialize, Serialize};

pub mod geometry;

use geometry::{CanvasParams, CropParams, FlipParams, ResizeParams, RotateParams};

use crate::{
    ErrorCode, PicError, Result,
    document::{Raster, TargetId},
    limits::ResourceLimits,
    pipeline::ResourceResolver,
};

pub const OP_VERSION: u32 = 1;

/// Authoring wire format. Versions, targets and sampling/background parameters are explicit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationSpec {
    pub op: String,
    pub op_version: u32,
    pub target: TargetId,
    pub params: serde_json::Value,
}

impl OperationSpec {
    pub fn identity() -> Self {
        Self {
            op: "identity".into(),
            op_version: OP_VERSION,
            target: TargetId::canvas(),
            params: serde_json::json!({}),
        }
    }

    pub fn validate(&self) -> Result<Operation> {
        if !["identity", "crop", "resize", "rotate", "flip", "canvas"].contains(&self.op.as_str()) {
            return Err(PicError::new(
                ErrorCode::UnknownOperation,
                format!("operation '{}' is not implemented", self.op),
            ));
        }
        if self.op_version != OP_VERSION {
            return Err(PicError::new(
                ErrorCode::UnsupportedVersion,
                format!(
                    "unsupported {} op_version {}; expected {OP_VERSION}",
                    self.op, self.op_version
                ),
            ));
        }
        if self.target != TargetId::canvas() {
            return Err(PicError::new(
                ErrorCode::InvalidTarget,
                "stateless operations require stable target 'canvas'",
            ));
        }
        let operation = match self.op.as_str() {
            "identity" => {
                if !self
                    .params
                    .as_object()
                    .is_some_and(|params| params.is_empty())
                {
                    return Err(PicError::new(
                        ErrorCode::InvalidArgument,
                        "identity params must be an empty object",
                    ));
                }
                Operation::Identity
            }
            "crop" => Operation::Crop(self.parameters()?),
            "resize" => Operation::Resize(self.parameters()?),
            "rotate" => Operation::Rotate(self.parameters()?),
            "flip" => Operation::Flip(self.parameters()?),
            "canvas" => Operation::Canvas(self.parameters()?),
            _ => unreachable!("known operation checked above"),
        };
        operation.validate()?;
        Ok(operation)
    }

    fn parameters<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_value(self.params.clone()).map_err(|e| {
            PicError::new(
                ErrorCode::InvalidArgument,
                format!("{} params: {e}", self.op),
            )
        })
    }
}

/// Validated executable operations. Future commands must use this same dispatcher.
#[derive(Debug, Clone)]
pub enum Operation {
    Identity,
    Crop(CropParams),
    Resize(ResizeParams),
    Rotate(RotateParams),
    Flip(FlipParams),
    Canvas(CanvasParams),
}

impl Operation {
    /// Explicit, serializable parameters for CLI authoring and future immutable ops.
    pub fn to_spec(&self) -> OperationSpec {
        let (op, params) = match self {
            Self::Identity => ("identity", serde_json::json!({})),
            Self::Crop(p) => ("crop", serde_json::json!(p)),
            Self::Resize(p) => ("resize", serde_json::json!(p)),
            Self::Rotate(p) => ("rotate", serde_json::json!(p)),
            Self::Flip(p) => ("flip", serde_json::json!(p)),
            Self::Canvas(p) => ("canvas", serde_json::json!(p)),
        };
        OperationSpec {
            op: op.into(),
            op_version: OP_VERSION,
            target: TargetId::canvas(),
            params,
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Identity | Self::Flip(_) => Ok(()),
            Self::Crop(p) => p.validate(),
            Self::Resize(p) => p.validate(),
            Self::Rotate(p) => p.validate(),
            Self::Canvas(p) => p.validate(),
        }
    }

    /// One logical operation boundary, with no codec, quantization, or file publication.
    pub fn apply(&self, raster: &mut Raster, resources: &ResourceResolver) -> Result<()> {
        self.apply_with_limits(raster, resources, &ResourceLimits::default())
    }

    pub fn apply_with_limits(
        &self,
        raster: &mut Raster,
        _resources: &ResourceResolver,
        limits: &ResourceLimits,
    ) -> Result<()> {
        self.validate()?;
        limits.check_dimensions(raster.width(), raster.height())?;
        let result = match self {
            Self::Identity => return Ok(()),
            Self::Crop(p) => geometry::crop(raster, p, limits)?,
            Self::Resize(p) => geometry::resize(raster, p, limits)?,
            Self::Rotate(p) => geometry::rotate(raster, p, limits)?,
            Self::Flip(p) => geometry::flip(raster, p, limits)?,
            Self::Canvas(p) => geometry::canvas(raster, p, limits)?,
        };
        *raster = result;
        Ok(())
    }
}
