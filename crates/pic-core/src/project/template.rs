//! Portable authoring recipe, never a revision or saved content-dependent result.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ErrorCode, PicError, Result, codec,
    document::{PIXEL_SEMANTICS, TargetId},
    limits::{ResourceLimits, read_limited},
    operation::OperationSpec,
    pipeline::{PIPELINE_SCHEMA_VERSION, Pipeline, PipelineSpec},
    result::{Diagnostics, timed},
};

use super::{ApplyRequest, Project, ProjectChange, serialize, storage};

const TEMPLATE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateStep {
    pub op: String,
    pub op_version: u32,
    pub target_slot: String,
    pub params_slot: String,
    /// Documentation for the caller; never substituted for a missing binding.
    pub suggested_params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationTemplate {
    pub schema_version: u32,
    pub pixel_semantics: String,
    pub input_slot: String,
    pub target_slots: Vec<String>,
    pub operations: Vec<TemplateStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateBindings {
    pub schema_version: u32,
    /// Paths resolve relative to this bindings file, not the source project/template.
    pub inputs: BTreeMap<String, PathBuf>,
    pub targets: BTreeMap<String, TargetId>,
    /// Every slot is mandatory, including empty parameter objects.
    pub params: BTreeMap<String, Value>,
}

pub struct TemplateRunRequest<'a> {
    pub template: &'a Path,
    pub bindings: &'a Path,
    /// Always a new project; its revisions are local to this new history.
    pub output: &'a Path,
}

#[derive(Debug, Serialize)]
pub struct TemplateExport {
    pub output: PathBuf,
    pub template: OperationTemplate,
}

fn invalid(message: impl Into<String>) -> PicError {
    PicError::new(ErrorCode::InvalidArgument, message)
}

impl OperationTemplate {
    fn bind(
        &self,
        bindings: &TemplateBindings,
        base: &Path,
        limits: &ResourceLimits,
    ) -> Result<(PathBuf, Pipeline)> {
        if self.schema_version != TEMPLATE_VERSION
            || bindings.schema_version != TEMPLATE_VERSION
            || self.pixel_semantics != PIXEL_SEMANTICS
        {
            return Err(PicError::new(
                ErrorCode::UnsupportedVersion,
                "unsupported template/bindings schema or pixel semantics",
            ));
        }
        if self.input_slot != "input"
            || self.target_slots != ["canvas"]
            || bindings.inputs.len() != 1
            || !bindings.inputs.contains_key("input")
            || bindings.targets.len() != 1
            || bindings.targets.get("canvas") != Some(&TargetId::canvas())
            || bindings.params.len() != self.operations.len()
        {
            return Err(invalid(
                "template requires explicit input, canvas target and every step parameter binding; extra bindings are rejected",
            ));
        }
        if self.operations.len() > limits.max_operations {
            return Err(PicError::new(
                ErrorCode::ResourceLimit,
                "template exceeds operation limit",
            ));
        }
        let mut operations = Vec::new();
        for (index, step) in self.operations.iter().enumerate() {
            let slot = format!("step_{}", index + 1);
            if step.params_slot != slot || step.target_slot != "canvas" {
                return Err(invalid(
                    "template slots must use canvas and ordered step_N parameters",
                ));
            }
            // Reject new resource/state operations on import too, including crafted templates.
            if !(OperationSpec {
                op: step.op.clone(),
                op_version: step.op_version,
                target: TargetId::canvas(),
                params: step.suggested_params.clone(),
            })
            .validate()?
            .template_safe()
            {
                return Err(PicError::new(
                    ErrorCode::UnsupportedTemplate,
                    "template operation requires layer/asset/selection rebinding support",
                ));
            }
            let params = bindings
                .params
                .get(&slot)
                .ok_or_else(|| invalid(format!("missing required parameter binding '{slot}'")))?;
            operations.push(OperationSpec {
                op: step.op.clone(),
                op_version: step.op_version,
                target: bindings.targets["canvas"].clone(),
                params: params.clone(),
            });
        }
        let input = &bindings.inputs["input"];
        if input.as_os_str().is_empty() {
            return Err(invalid("input binding must name a file"));
        }
        Ok((
            base.join(input),
            Pipeline::new(
                PipelineSpec {
                    schema_version: PIPELINE_SCHEMA_VERSION,
                    operations,
                },
                base,
                limits,
            )?,
        ))
    }
}

impl Project {
    pub fn operation_template(&self, revision: Option<&str>) -> Result<OperationTemplate> {
        let steps = self
            .history
            .path(revision.unwrap_or(&self.manifest.current_revision.0))?;
        let mut operations = Vec::new();
        for (index, step) in steps.iter().enumerate() {
            // Fail closed when future history adds masks, fonts, content selections or model results.
            // Those operations need an explicit asset/coordinate rebinding policy before export.
            if !step.result_assets.is_empty() || step.input_assets != [self.manifest.source.clone()]
            {
                return Err(PicError::new(
                    ErrorCode::UnsupportedTemplate,
                    "asset-dependent operations require explicit rebinding support before template export",
                ));
            }
            if step.operation.target != TargetId::canvas()
                || !step.operation.validate()?.template_safe()
            {
                return Err(PicError::new(
                    ErrorCode::UnsupportedTemplate,
                    "layers, masks, selections and composite require explicit rebinding support before template export",
                ));
            }
            operations.push(TemplateStep {
                op: step.operation.op.clone(),
                op_version: step.operation.op_version,
                target_slot: "canvas".into(),
                params_slot: format!("step_{}", index + 1),
                suggested_params: step.operation.params.clone(),
            });
        }
        Ok(OperationTemplate {
            schema_version: TEMPLATE_VERSION,
            pixel_semantics: PIXEL_SEMANTICS.into(),
            input_slot: "input".into(),
            target_slots: vec!["canvas".into()],
            operations,
        })
    }

    pub fn export_template(
        &self,
        revision: Option<&str>,
        output: &Path,
        overwrite: bool,
        diagnostics: &mut Diagnostics,
    ) -> Result<TemplateExport> {
        let (template, output, bytes) = timed(&mut diagnostics.timings.validation_ms, || {
            let template = self.operation_template(revision)?;
            let output = codec::prepare_output(output, overwrite)?;
            if output.starts_with(self.path()) {
                return Err(PicError::new(
                    ErrorCode::UnsafePath,
                    "template output must be outside the project directory",
                ));
            }
            let bytes = serialize(&template, self.limits.max_pipeline_bytes)?;
            Ok((template, output, bytes))
        })?;
        timed(&mut diagnostics.timings.write_ms, || {
            codec::publish(&output, &bytes, overwrite)
        })?;
        Ok(TemplateExport { output, template })
    }

    pub fn run_template(
        request: TemplateRunRequest<'_>,
        limits: &ResourceLimits,
        diagnostics: &mut Diagnostics,
    ) -> Result<ProjectChange> {
        let (input, pipeline, count, destination) =
            timed(&mut diagnostics.timings.validation_ms, || {
                let template: OperationTemplate = serde_json::from_slice(&read_limited(
                    request.template,
                    limits.max_pipeline_bytes,
                )?)
                .map_err(|e| PicError::new(ErrorCode::InvalidJson, e.to_string()))?;
                let bindings_path = fs::canonicalize(request.bindings)
                    .map_err(|e| PicError::io("resolve bindings", request.bindings, e))?;
                let bindings: TemplateBindings = serde_json::from_slice(&read_limited(
                    &bindings_path,
                    limits.max_pipeline_bytes,
                )?)
                .map_err(|e| PicError::new(ErrorCode::InvalidJson, e.to_string()))?;
                let base = bindings_path
                    .parent()
                    .ok_or_else(|| invalid("bindings need a parent directory"))?;
                let (input, pipeline) = template.bind(&bindings, base, limits)?;
                let destination = storage::project_path(request.output)?;
                if destination.extension().is_none_or(|ext| ext != "pic") {
                    return Err(invalid("template output must be a new .pic directory"));
                }
                match fs::symlink_metadata(&destination) {
                    Ok(_) => {
                        return Err(PicError::new(
                            ErrorCode::OutputExists,
                            "template output project already exists",
                        ));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(PicError::io("inspect template output", &destination, e)),
                }
                Ok((input, pipeline, template.operations.len(), destination))
            })?;
        // Use the same project creation/commit path, then atomically publish the complete project.
        // A failed binding, operation, or final rename leaves no partial destination history.
        let staging = tempfile::Builder::new()
            .prefix(".pic-template-")
            .tempdir_in(
                destination
                    .parent()
                    .ok_or_else(|| invalid("output needs a parent"))?,
            )
            .map_err(|e| PicError::io("create template staging", &destination, e))?;
        let staged = staging.path().join("new.pic");
        let mut change = Self::create(&input, &staged, limits, diagnostics)?;
        if count != 0 {
            change = Self::apply(
                ApplyRequest {
                    project: &staged,
                    pipeline: &pipeline,
                    expected_revision: super::INITIAL_REVISION,
                    revision: None,
                },
                limits,
                diagnostics,
            )?;
        }
        timed(&mut diagnostics.timings.write_ms, || {
            storage::publish_directory(&staged, &destination)
        })?;
        change.project = destination;
        Ok(change)
    }
}
