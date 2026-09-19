use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    ErrorCode, PicError, Result,
    codec::{self, EncodeOptions, ImageInfo},
    document::{Document, Raster, TargetId},
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

/// Resolve external resource operands relative to the canonical pipeline file's directory.
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

    pub fn load(&self, source: &str, mask: bool, limits: &ResourceLimits) -> Result<Raster> {
        if source.starts_with("asset:") {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "asset references require a project asset store",
            ));
        }
        let path = self.resolve(Path::new(source))?;
        let bytes = read_limited(&path, limits.max_input_bytes)?;
        if mask {
            codec::load_mask_bytes(&bytes, path, limits)
        } else {
            Ok(codec::load_bytes(&bytes, path, limits, &mut Diagnostics::default())?.raster)
        }
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
    limits: ResourceLimits,
}

#[derive(Debug, Serialize)]
pub struct ExecutedStep {
    /// Zero-based logical index. No fusion or reordering is performed.
    pub index: usize,
    pub op: String,
    pub op_version: u32,
    pub target: TargetId,
    pub params: serde_json::Value,
}

#[derive(Debug)]
pub struct Execution {
    pub raster: Raster,
    pub document: Document,
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
                Ok((spec.normalized()?, operation))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            steps,
            resources: ResourceResolver::new(base_dir)?,
            limits: limits.clone(),
        })
    }

    pub fn resources(&self) -> &ResourceResolver {
        &self.resources
    }

    pub fn specs(&self) -> impl Iterator<Item = &OperationSpec> {
        self.steps.iter().map(|(spec, _)| spec)
    }

    /// Every path evaluates the same Document; one-shot output is its rendered canvas.
    pub fn execute(&self, raster: Raster) -> Result<Execution> {
        self.execute_with_limits(raster, &self.limits)
    }

    pub fn execute_with_limits(
        &self,
        raster: Raster,
        limits: &ResourceLimits,
    ) -> Result<Execution> {
        let limits = self.effective_limits(limits);
        self.execute_document(
            Document::from_raster(raster),
            &limits,
            &mut |source, mask| self.resources.load(source, mask, &limits),
        )
    }

    pub(crate) fn effective_limits(&self, limits: &ResourceLimits) -> ResourceLimits {
        ResourceLimits {
            max_dimension: limits.max_dimension.min(self.limits.max_dimension),
            max_pixels: limits.max_pixels.min(self.limits.max_pixels),
            max_input_bytes: limits.max_input_bytes.min(self.limits.max_input_bytes),
            max_operations: limits.max_operations.min(self.limits.max_operations),
            max_buffer_bytes: limits.max_buffer_bytes.min(self.limits.max_buffer_bytes),
            ..limits.clone()
        }
    }

    pub(crate) fn execute_document(
        &self,
        mut document: Document,
        limits: &ResourceLimits,
        load: &mut impl FnMut(&str, bool) -> Result<Raster>,
    ) -> Result<Execution> {
        if self.steps.len() > limits.max_operations {
            return Err(PicError::new(
                ErrorCode::ResourceLimit,
                "pipeline exceeds operation limit",
            ));
        }
        let limits = self.effective_limits(limits);
        document.validate(&limits)?;
        let mut steps = Vec::with_capacity(self.steps.len());
        for (index, (spec, operation)) in self.steps.iter().enumerate() {
            document
                .apply(&spec.target, operation, &self.resources, &limits, load)
                .map_err(|mut error| {
                    error.message = format!("operations[{index}]: {}", error.message);
                    error
                })?;
            steps.push(ExecutedStep {
                index,
                op: spec.op.clone(),
                op_version: spec.op_version,
                target: spec.target.clone(),
                params: spec.params.clone(),
            });
        }
        let raster = document.render(&TargetId::canvas(), &limits)?;
        Ok(Execution {
            raster,
            document,
            steps,
        })
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
    pub png_compression: Option<u8>,
    pub jpeg_background: Option<crate::operation::geometry::RgbaColor>,
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
        request.pipeline.execute_with_limits(input.raster, limits)
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
    if execution.raster.has_transparency() && encoding.jpeg_background.is_some() {
        diagnostics.warnings.push(crate::result::Warning {
            code: "alpha_flattened",
            message: "JPEG export composites transparency over the explicit background in linear sRGB.",
        });
    }
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
        png_compression: encoding.png_compression,
        jpeg_background: encoding.jpeg_background,
    })
}
