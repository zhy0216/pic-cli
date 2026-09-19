//! Bind external files once into content-addressed assets before committing semantic ops.
use super::{AssetRef, Project, invalid, storage};
use crate::{
    ErrorCode, PicError, Result, codec,
    document::Raster,
    limits::{ResourceLimits, read_limited},
    operation::OperationSpec,
    pipeline::{PIPELINE_SCHEMA_VERSION, Pipeline, PipelineSpec},
    result::{Diagnostics, timed},
};
use std::path::Path;

pub(super) fn digest(source: &str) -> Result<&str> {
    let hash = source.strip_prefix("asset:").ok_or_else(|| {
        PicError::new(
            ErrorCode::UnsafePath,
            "committed resource operands must be asset:<sha256>, never external paths",
        )
    })?;
    storage::check_hash(hash)?;
    Ok(hash)
}

pub(super) fn validate_dependencies(
    operation: &OperationSpec,
    inputs: &[AssetRef],
    source: &AssetRef,
) -> Result<()> {
    let mut expected = vec![source.clone()];
    for operand in operation.validate()?.sources() {
        let hash = digest(operand)?;
        if expected.iter().any(|asset| asset.sha256 == hash) {
            continue;
        }
        let asset = inputs
            .iter()
            .find(|a| a.sha256 == hash)
            .ok_or_else(|| invalid("operation asset operand has no dependency reference"))?;
        expected.push(asset.clone());
    }
    if expected != inputs {
        return Err(invalid(
            "operation dependencies must be source plus distinct resource operands in order",
        ));
    }
    Ok(())
}

impl Project {
    pub(super) fn load_font(&self, source: &str, limits: &ResourceLimits) -> Result<Vec<u8>> {
        self.storage.asset(digest(source)?, limits.max_input_bytes)
    }

    pub(super) fn load_operand(
        &self,
        source: &str,
        mask: bool,
        limits: &ResourceLimits,
    ) -> Result<Raster> {
        let hash = digest(source)?;
        let bytes = self.storage.asset(hash, limits.max_input_bytes)?;
        let path = self.path().join("assets").join(hash);
        if mask {
            codec::load_mask_bytes(&bytes, path, limits)
        } else {
            Ok(codec::load_bytes(&bytes, path, limits, &mut Diagnostics::default())?.raster)
        }
    }

    pub(super) fn bind_pipeline(
        &self,
        pipeline: &Pipeline,
        diagnostics: &mut Diagnostics,
    ) -> Result<(Pipeline, Vec<Vec<AssetRef>>)> {
        let limits = pipeline.effective_limits(&self.limits);
        let mut dependencies = Vec::new();
        let mut operations = Vec::new();
        for spec in pipeline.specs() {
            let mut operation = spec.validate()?;
            let mut inputs = vec![self.manifest.source.clone()];
            operation.bind_sources(|source| {
                let bytes = timed(&mut diagnostics.timings.read_ms, || {
                    if source.starts_with("asset:") {
                        self.storage.asset(digest(source)?, limits.max_input_bytes)
                    } else {
                        read_limited(
                            &pipeline.resources().resolve(Path::new(source))?,
                            limits.max_input_bytes,
                        )
                    }
                })?;
                let asset = AssetRef {
                    sha256: timed(&mut diagnostics.timings.write_ms, || {
                        self.storage.put_asset(&bytes)
                    })?,
                    bytes: bytes.len() as u64,
                };
                let bound = format!("asset:{}", asset.sha256);
                if !inputs.contains(&asset) {
                    inputs.push(asset);
                }
                Ok(bound)
            })?;
            let mut normalized = operation.to_spec();
            normalized.target = spec.target.clone();
            operations.push(normalized);
            dependencies.push(inputs);
        }
        Ok((
            Pipeline::new(
                PipelineSpec {
                    schema_version: PIPELINE_SCHEMA_VERSION,
                    operations,
                },
                self.path(),
                &limits,
            )?,
            dependencies,
        ))
    }
}
