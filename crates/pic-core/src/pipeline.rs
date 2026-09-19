use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    ErrorCode, PicError, Result,
    codec::{self, EncodeOptions, ImageInfo},
    document::{Raster, TargetId},
    limits::{ResourceLimits, read_limited},
    operation::{Operation, OperationSpec},
    result::{Diagnostics, timed},
};

pub const PIPELINE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineSpec {
    pub schema_version: u32,
    pub operations: Vec<OperationSpec>,
}

/// Resolve future resource operands relative to the canonical pipeline file's directory.
/// CLI input/output paths are deliberately not resolved through this object.
#[derive(Debug, Clone)]
pub struct ResourceResolver {
    base_dir: PathBuf,
}

impl ResourceResolver {
    pub fn new(base_dir: &Path) -> Result<Self> {
        let base_dir = fs::canonicalize(base_dir)
            .map_err(|e| PicError::io("resolve resource directory", base_dir, e))?;
        if !base_dir.is_dir() {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "resource base must be a directory",
            ));
        }
        if base_dir.to_str().is_none() {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "resource directory must be valid UTF-8",
            ));
        }
        Ok(Self { base_dir })
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Absolute paths stay absolute; `..` is permitted. This is not a filesystem sandbox.
    pub fn resolve(&self, path: &Path) -> Result<PathBuf> {
        if path.as_os_str().is_empty() {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "resource path cannot be empty",
            ));
        }
        Ok(self.base_dir.join(path))
    }
}

#[derive(Debug)]
pub struct Pipeline {
    steps: Vec<(OperationSpec, Operation)>,
    resources: ResourceResolver,
}

#[derive(Debug, Serialize)]
pub struct ExecutedStep {
    /// Zero-based logical index. No fusion or reordering is performed.
    pub index: usize,
    pub op: String,
    pub op_version: u32,
    pub target: TargetId,
}

#[derive(Debug)]
pub struct Execution {
    pub raster: Raster,
    pub steps: Vec<ExecutedStep>,
}

impl Pipeline {
    pub fn from_file(path: &Path, limits: &ResourceLimits) -> Result<Self> {
        let path = fs::canonicalize(path).map_err(|e| PicError::io("resolve pipeline", path, e))?;
        let bytes = read_limited(&path, limits.max_pipeline_bytes)?;
        let base = path.parent().ok_or_else(|| {
            PicError::new(
                ErrorCode::InvalidArgument,
                "pipeline has no parent directory",
            )
        })?;
        Self::from_json(&bytes, base, limits)
    }

    pub fn from_json(bytes: &[u8], base_dir: &Path, limits: &ResourceLimits) -> Result<Self> {
        if bytes.len() as u64 > limits.max_pipeline_bytes {
            return Err(PicError::new(
                ErrorCode::ResourceLimit,
                "pipeline exceeds byte limit",
            ));
        }
        let spec = serde_json::from_slice(bytes)
            .map_err(|e| PicError::new(ErrorCode::InvalidJson, e.to_string()))?;
        Self::new(spec, base_dir, limits)
    }

    pub fn single(
        operation: OperationSpec,
        base_dir: &Path,
        limits: &ResourceLimits,
    ) -> Result<Self> {
        Self::new(
            PipelineSpec {
                schema_version: PIPELINE_SCHEMA_VERSION,
                operations: vec![operation],
            },
            base_dir,
            limits,
        )
    }

    pub fn new(spec: PipelineSpec, base_dir: &Path, limits: &ResourceLimits) -> Result<Self> {
        if spec.schema_version != PIPELINE_SCHEMA_VERSION {
            return Err(PicError::new(
                ErrorCode::UnsupportedVersion,
                format!(
                    "unsupported pipeline schema_version {}; expected {PIPELINE_SCHEMA_VERSION}",
                    spec.schema_version
                ),
            ));
        }
        if spec.operations.len() > limits.max_operations {
            return Err(PicError::new(
                ErrorCode::ResourceLimit,
                "pipeline exceeds operation limit",
            ));
        }
        let steps = spec
            .operations
            .into_iter()
            .enumerate()
            .map(|(index, spec)| {
                let operation = spec.validate().map_err(|mut error| {
                    error.message = format!("operations[{index}]: {}", error.message);
                    error
                })?;
                Ok((spec, operation))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            steps,
            resources: ResourceResolver::new(base_dir)?,
        })
    }

    pub fn resources(&self) -> &ResourceResolver {
        &self.resources
    }

    /// Retains every logical step boundary and never changes pixel representation.
    pub fn execute(&self, mut raster: Raster) -> Result<Execution> {
        let mut steps = Vec::with_capacity(self.steps.len());
        for (index, (spec, operation)) in self.steps.iter().enumerate() {
            operation.apply(&mut raster, &self.resources)?;
            steps.push(ExecutedStep {
                index,
                op: spec.op.clone(),
                op_version: spec.op_version,
                target: spec.target.clone(),
            });
        }
        Ok(Execution { raster, steps })
    }
}

pub struct RunRequest<'a> {
    pub input: &'a Path,
    pub output: &'a Path,
    pub pipeline: &'a Pipeline,
    pub encoding: EncodeOptions,
    pub overwrite: bool,
}

#[derive(Debug, Serialize)]
pub struct RunResult {
    pub input: ImageInfo,
    pub output: PathBuf,
    pub format: codec::Format,
    pub width: u32,
    pub height: u32,
    pub operations_applied: usize,
    pub steps: Vec<ExecutedStep>,
    pub resource_base: PathBuf,
    pub jpeg_quality: Option<u8>,
}

/// The only file-processing entry point for both a single operation and JSON pipelines.
pub fn run(
    request: RunRequest<'_>,
    limits: &ResourceLimits,
    diagnostics: &mut Diagnostics,
) -> Result<RunResult> {
    let (destination, encoding) = timed(&mut diagnostics.timings.validation_ms, || {
        let encoding = request.encoding.resolve(request.output)?;
        let destination = codec::prepare_output(request.output, request.overwrite)?;
        Ok((destination, encoding))
    })?;
    let input = codec::load(request.input, limits, diagnostics)?;
    let execution = timed(&mut diagnostics.timings.process_ms, || {
        request.pipeline.execute(input.raster)
    })?;
    let encoded = timed(&mut diagnostics.timings.encode_ms, || {
        codec::encode(&execution.raster, &encoding, limits)
    })?;
    timed(&mut diagnostics.timings.write_ms, || {
        codec::publish(&destination, &encoded, request.overwrite)
    })?;
    diagnostics.warnings.push(crate::result::Warning {
        code: "metadata_not_preserved",
        message: "Export writes pixels only; source metadata is not preserved.",
    });
    Ok(RunResult {
        input: input.info,
        output: destination,
        format: encoding.format,
        width: execution.raster.width(),
        height: execution.raster.height(),
        operations_applied: execution.steps.len(),
        steps: execution.steps,
        resource_base: request.pipeline.resources.base_dir.clone(),
        jpeg_quality: encoding.jpeg_quality,
    })
}
