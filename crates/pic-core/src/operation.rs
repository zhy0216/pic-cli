use serde::{Deserialize, Serialize};

use crate::{
    ErrorCode, PicError, Result,
    document::{Raster, TargetId},
    pipeline::ResourceResolver,
};

pub const OP_VERSION: u32 = 1;

/// Authoring wire format. No omitted versions, targets, or parameter defaults in v1.
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
        if self.op != "identity" {
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
        Ok(Operation::Identity)
    }
}

/// Validated executable operations. Future commands must use this same dispatcher.
#[derive(Debug, Clone)]
pub enum Operation {
    Identity,
}

impl Operation {
    /// One logical operation boundary, with no codec, quantization, or file publication.
    pub fn apply(&self, _raster: &mut Raster, _resources: &ResourceResolver) -> Result<()> {
        match self {
            Self::Identity => Ok(()),
        }
    }
}
