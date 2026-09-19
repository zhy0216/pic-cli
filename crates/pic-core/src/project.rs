//! Self-contained source assets and immutable semantic ops. No raster snapshot is authoritative.
//! Readers use one manifest snapshot; writers hold the stable `.lock` through publication.

mod assets;
mod cache;
mod history;
mod preview;
mod snapshot;
mod storage;
mod template;

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::{
    ErrorCode, PicError, Result,
    codec::{self, EncodeOptions},
    document::{
        DOCUMENT_SCHEMA_VERSION, Document, DocumentInfo, PIXEL_SEMANTICS, Raster, RevisionId,
        TargetId,
    },
    limits::{ResourceLimits, read_limited},
    pipeline::{PIPELINE_SCHEMA_VERSION, Pipeline, PipelineSpec},
    result::{Diagnostics, Warning, timed},
};

pub use cache::{CacheClearResult, CacheHit, CheckpointResult, ImageSize, ReplayReport};
pub use history::{AssetRef, Commit, CommitRef, CommittedOp, INITIAL_REVISION, Manifest};
use history::{History, invalid};
pub use preview::{AffineMapping, CoordinateMapping, PreviewRequest, ProjectPreview};
use storage::Storage;
pub use template::{
    OperationTemplate, TemplateBindings, TemplateExport, TemplateRunRequest, TemplateStep,
};

pub struct Project {
    storage: Storage,
    manifest: Manifest,
    manifest_bytes: Vec<u8>,
    history: History,
    limits: ResourceLimits,
    memory: std::cell::RefCell<cache::MemoryCache>,
}

pub struct RestoredRevision {
    pub document: Document,
    pub revision: RevisionId,
    pub raster: Raster,
    pub operations_replayed: usize,
    pub replay: ReplayReport,
}

#[derive(Debug, Serialize)]
pub struct ProjectChange {
    pub project: PathBuf,
    pub previous_revision: Option<RevisionId>,
    pub base_revision: Option<RevisionId>,
    pub revision: RevisionId,
    pub commit_id: Option<String>,
    pub steps: Vec<CommittedOp>,
    pub published: bool,
    pub width: u32,
    pub height: u32,
    pub replay: ReplayReport,
}

#[derive(Debug, Serialize)]
pub struct ProjectInspection {
    pub document: DocumentInfo,
    pub project: PathBuf,
    pub revision: RevisionId,
    pub current_revision: RevisionId,
    pub target: TargetId,
    pub op_id: Option<String>,
    pub commit_id: Option<String>,
    pub width: u32,
    pub height: u32,
    pub operations_replayed: usize,
    pub replay: ReplayReport,
    pub active_revisions: Vec<String>,
    pub group_boundaries: Vec<String>,
    pub manifest: Manifest,
    pub commits: Vec<Commit>,
}

pub struct ApplyRequest<'a> {
    pub project: &'a Path,
    pub pipeline: &'a Pipeline,
    /// Always compares against the current pointer, even when editing an old step.
    pub expected_revision: &'a str,
    pub revision: Option<&'a str>,
}

pub struct ExportRequest<'a> {
    pub revision: Option<&'a str>,
    pub output: &'a Path,
    pub encoding: EncodeOptions,
    pub overwrite: bool,
}

pub struct ReviseRequest<'a> {
    pub project: &'a Path,
    /// A step on the current revision's ancestry, never a numeric position.
    pub step_revision: &'a str,
    pub params: serde_json::Value,
    pub expected_revision: &'a str,
}

#[derive(Debug, Serialize)]
pub struct ProjectExport {
    pub project: PathBuf,
    pub revision: RevisionId,
    pub target: TargetId,
    pub op_id: Option<String>,
    pub output: PathBuf,
    pub width: u32,
    pub height: u32,
    pub operations_replayed: usize,
    pub replay: ReplayReport,
    pub format: codec::Format,
    pub jpeg_quality: Option<u8>,
    pub png_compression: Option<u8>,
    pub jpeg_background: Option<crate::operation::geometry::RgbaColor>,
}

impl Project {
    pub fn create(
        input: &Path,
        path: &Path,
        limits: &ResourceLimits,
        diagnostics: &mut Diagnostics,
    ) -> Result<ProjectChange> {
        let destination = storage::project_path(path)?;
        if destination.extension().is_none_or(|ext| ext != "pic") {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "new project directory must have a .pic extension",
            ));
        }
        match fs::symlink_metadata(&destination) {
            Ok(_) => {
                return Err(PicError::new(
                    ErrorCode::OutputExists,
                    "project destination already exists",
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(PicError::io("inspect project destination", &destination, e)),
        }
        let (input, bytes) = timed(&mut diagnostics.timings.read_ms, || {
            let input = fs::canonicalize(input)
                .map_err(|e| PicError::io("resolve project source", input, e))?;
            let bytes = read_limited(&input, limits.max_input_bytes)?;
            Ok((input, bytes))
        })?;
        let loaded = codec::load_bytes(&bytes, input.clone(), limits, diagnostics)?;
        let manifest = Manifest {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            pixel_semantics: PIXEL_SEMANTICS.into(),
            engine_version: env!("CARGO_PKG_VERSION").into(),
            source: AssetRef {
                sha256: storage::hash(&bytes),
                bytes: bytes.len() as u64,
            },
            imported_from: input
                .to_str()
                .ok_or_else(|| invalid("source path must be UTF-8"))?
                .into(),
            initial_revision: RevisionId(INITIAL_REVISION.into()),
            current_revision: RevisionId(INITIAL_REVISION.into()),
            tip_revision: RevisionId(INITIAL_REVISION.into()),
            commits: Vec::new(),
        };
        let manifest_bytes = serialize(&manifest, limits.max_project_bytes)?;
        timed(&mut diagnostics.timings.write_ms, || {
            let parent = destination
                .parent()
                .ok_or_else(|| invalid("project has no parent"))?;
            let staging = tempfile::Builder::new()
                .prefix(".pic-create-")
                .tempdir_in(parent)
                .map_err(|e| PicError::io("create project staging directory", parent, e))?;
            let storage = Storage::initialize(staging.path())?;
            storage.put_asset(&bytes)?;
            storage.publish_manifest(&manifest_bytes, true)?;
            storage::publish_directory(staging.path(), &destination)
        })?;
        Ok(ProjectChange {
            project: destination,
            previous_revision: None,
            base_revision: None,
            revision: manifest.current_revision,
            commit_id: None,
            steps: Vec::new(),
            published: true,
            width: loaded.raster.width(),
            height: loaded.raster.height(),
            replay: ReplayReport::default(),
        })
    }

    pub fn open(
        path: &Path,
        limits: &ResourceLimits,
        diagnostics: &mut Diagnostics,
    ) -> Result<Self> {
        let storage = Storage::open(path)?;
        Self::load(storage, limits, diagnostics)
    }

    fn load(
        storage: Storage,
        limits: &ResourceLimits,
        diagnostics: &mut Diagnostics,
    ) -> Result<Self> {
        let (manifest_bytes, manifest, history) = timed(&mut diagnostics.timings.read_ms, || {
            let bytes = storage.manifest(limits.max_project_bytes)?;
            let manifest = history::parse(&bytes)?;
            let history = History::load(&storage, &manifest, bytes.len() as u64, limits)?;
            Ok((bytes, manifest, history))
        })?;
        Ok(Self {
            storage,
            manifest,
            manifest_bytes,
            history,
            limits: limits.clone(),
            memory: cache::new_memory(),
        })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn commits(&self) -> &[Commit] {
        &self.history.commits
    }
    pub fn path(&self) -> &Path {
        &self.storage.path
    }

    /// Restore a validated exact checkpoint or replay through the existing pipeline.
    pub fn restore(
        &self,
        revision: Option<&str>,
        diagnostics: &mut Diagnostics,
    ) -> Result<RestoredRevision> {
        let revision = revision.unwrap_or(&self.manifest.current_revision.0);
        let steps = self.history.path(revision)?;
        let bytes = self.verified_source(&steps, diagnostics)?;
        let keys = self.prefix_keys(&steps)?;
        let mut replay = ReplayReport::default();
        let mut cached = None;
        // Shape is part of a valid checkpoint: a layered prefix can never resume from a flat image.
        let mut layered_prefix = vec![false];
        for step in &steps {
            layered_prefix.push(
                *layered_prefix.last().expect("initial state")
                    || step.operation.target != TargetId::canvas()
                    || matches!(
                        step.operation.validate()?,
                        crate::operation::Operation::Layer(_)
                    ),
            );
        }
        for index in (0..keys.len()).rev() {
            if let Some((value, tier)) =
                self.cache_get(storage::DerivedKind::Checkpoint, &keys[index], diagnostics)
            {
                // A checkpoint must retain the complete canvas, never a preview region.
                if value.canvas != ImageSize::of(&value.raster)
                    || value.document.is_some() != layered_prefix[index]
                    || value.layer_to_canvas.is_some()
                {
                    continue;
                }
                replay.reused_steps = index;
                replay.cache_hit = Some(CacheHit {
                    kind: "checkpoint",
                    tier,
                    key: keys[index].clone(),
                    revision: if index == 0 {
                        RevisionId(INITIAL_REVISION.into())
                    } else {
                        steps[index - 1].revision.clone()
                    },
                });
                cached = Some(
                    value
                        .document
                        .unwrap_or_else(|| Document::from_raster(value.raster)),
                );
                break;
            }
        }
        let input = match cached {
            Some(document) => document,
            None => Document::from_raster(
                codec::load_bytes(
                    &bytes,
                    self.path()
                        .join("assets")
                        .join(&self.manifest.source.sha256),
                    &self.limits,
                    diagnostics,
                )?
                .raster,
            ),
        };
        let document = timed(&mut diagnostics.timings.process_ms, || {
            let mut document = input;
            for chunk in steps[replay.reused_steps..].chunks(self.limits.max_operations.max(1)) {
                let pipeline = Pipeline::new(
                    PipelineSpec {
                        schema_version: PIPELINE_SCHEMA_VERSION,
                        operations: chunk.iter().map(|step| step.operation.clone()).collect(),
                    },
                    self.path(),
                    &self.limits,
                )?;
                document = pipeline
                    .execute_document(
                        document,
                        &self.limits,
                        &mut |source, mask| self.load_operand(source, mask, &self.limits),
                        &mut |source| self.load_font(source, &self.limits),
                    )?
                    .document;
                replay
                    .recomputed_revisions
                    .extend(chunk.iter().map(|step| step.revision.clone()));
            }
            Ok(document)
        })?;
        let raster = timed(&mut diagnostics.timings.process_ms, || {
            document.render(&TargetId::canvas(), &self.limits)
        })?;
        Ok(RestoredRevision {
            document,
            revision: RevisionId(revision.into()),
            raster,
            operations_replayed: replay.recomputed_revisions.len(),
            replay,
        })
    }

    pub fn inspect(
        &self,
        revision: Option<&str>,
        diagnostics: &mut Diagnostics,
    ) -> Result<ProjectInspection> {
        let restored = self.restore(revision, diagnostics)?;
        let step = self.history.step(&restored.revision.0);
        Ok(ProjectInspection {
            document: restored.document.info(),
            project: self.path().to_owned(),
            current_revision: self.manifest.current_revision.clone(),
            target: TargetId::canvas(),
            op_id: step.map(|(_, step)| step.op_id.clone()),
            commit_id: step.map(|(commit, _)| commit.commit_id.clone()),
            revision: restored.revision,
            width: restored.raster.width(),
            height: restored.raster.height(),
            operations_replayed: restored.operations_replayed,
            replay: restored.replay,
            active_revisions: self
                .history
                .active_revisions(&self.manifest.tip_revision.0)?
                .into_iter()
                .collect(),
            group_boundaries: self.history.boundaries(&self.manifest.tip_revision.0)?,
            manifest: self.manifest.clone(),
            commits: self.history.commits.clone(),
        })
    }

    pub fn apply(
        request: ApplyRequest<'_>,
        limits: &ResourceLimits,
        diagnostics: &mut Diagnostics,
    ) -> Result<ProjectChange> {
        let storage = Storage::open(request.project)?;
        let _lock = storage.lock()?;
        let project = Self::load(storage, limits, diagnostics)?;
        project.expect(request.expected_revision)?;
        let restored = project.restore(request.revision, diagnostics)?;
        project.commit_pipeline(
            restored,
            request.pipeline,
            request.expected_revision,
            diagnostics,
        )
    }

    /// Replace parameters at a logical step and commit a freshly evaluated suffix as one group.
    pub fn revise(
        request: ReviseRequest<'_>,
        limits: &ResourceLimits,
        diagnostics: &mut Diagnostics,
    ) -> Result<ProjectChange> {
        let storage = Storage::open(request.project)?;
        let _lock = storage.lock()?;
        let project = Self::load(storage, limits, diagnostics)?;
        project.expect(request.expected_revision)?;
        let path = project.history.path(&project.manifest.current_revision.0)?;
        let index = path
            .iter()
            .position(|step| step.revision.0 == request.step_revision)
            .ok_or_else(|| {
                PicError::new(
                    ErrorCode::RevisionNotFound,
                    "step revision is not on the current history",
                )
            })?;
        let mut operations: Vec<_> = path[index..]
            .iter()
            .map(|step| step.operation.clone())
            .collect();
        operations[0].params = request.params;
        let pipeline = Pipeline::new(
            PipelineSpec {
                schema_version: PIPELINE_SCHEMA_VERSION,
                operations,
            },
            project.path(),
            limits,
        )?;
        let restored = project.restore(Some(&path[index].base_revision.0), diagnostics)?;
        project.commit_pipeline(restored, &pipeline, request.expected_revision, diagnostics)
    }

    fn commit_pipeline(
        &self,
        restored: RestoredRevision,
        pipeline: &Pipeline,
        expected: &str,
        diagnostics: &mut Diagnostics,
    ) -> Result<ProjectChange> {
        let project = self;
        let limits = &self.limits;
        let execution_limits = pipeline.effective_limits(limits);
        let (pipeline, dependencies) = self.bind_pipeline(pipeline, diagnostics)?;
        let execution = timed(&mut diagnostics.timings.process_ms, || {
            pipeline.execute_document(
                restored.document,
                limits,
                &mut |source, mask| self.load_operand(source, mask, &execution_limits),
                &mut |source| self.load_font(source, &execution_limits),
            )
        })?;
        if execution.steps.is_empty() {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "project apply requires at least one operation",
            ));
        }
        if execution.steps.len()
            > limits
                .max_history_operations
                .saturating_sub(project.history.len())
        {
            return Err(PicError::new(
                ErrorCode::ResourceLimit,
                "project exceeds history operation limit",
            ));
        }
        let base_revision = restored.revision;
        let mut base = base_revision.clone();
        let steps: Vec<_> = execution
            .steps
            .into_iter()
            .enumerate()
            .map(|(index, step)| {
                let number = project.history.len() + index + 1;
                let revision = RevisionId(format!("r{number}"));
                let committed = CommittedOp {
                    op_id: format!("op{number}"),
                    base_revision: base.clone(),
                    revision: revision.clone(),
                    operation: crate::operation::OperationSpec {
                        op: step.op,
                        op_version: step.op_version,
                        target: step.target,
                        params: step.params,
                    },
                    input_assets: dependencies[index].clone(),
                    result_assets: Vec::new(),
                };
                base = revision;
                committed
            })
            .collect();
        let commit = Commit {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            commit_id: format!("c{}", project.manifest.commits.len() + 1),
            base_revision: base_revision.clone(),
            revision: base.clone(),
            steps,
        };
        let commit_bytes = serialize(&commit, limits.max_project_bytes)?;
        let mut manifest = project.manifest.clone();
        manifest.commits.push(CommitRef {
            commit_id: commit.commit_id.clone(),
            sha256: storage::hash(&commit_bytes),
        });
        manifest.current_revision = base.clone();
        manifest.tip_revision = base.clone();
        let manifest_bytes = serialize(&manifest, limits.max_project_bytes)?;
        project.check_metadata_budget(&manifest_bytes, commit_bytes.len() as u64)?;
        timed(&mut diagnostics.timings.write_ms, || {
            project.storage.put_commit(&commit_bytes)?;
            project.publish(&manifest_bytes, expected)
        })?;
        let mut replay = restored.replay;
        replay
            .recomputed_revisions
            .extend(commit.steps.iter().map(|step| step.revision.clone()));
        Ok(ProjectChange {
            project: project.path().to_owned(),
            previous_revision: Some(project.manifest.current_revision.clone()),
            base_revision: Some(base_revision),
            revision: base,
            commit_id: Some(commit.commit_id),
            steps: commit.steps,
            published: true,
            width: execution.raster.width(),
            height: execution.raster.height(),
            replay,
        })
    }

    pub fn undo(
        path: &Path,
        expected_revision: &str,
        limits: &ResourceLimits,
        diagnostics: &mut Diagnostics,
    ) -> Result<ProjectChange> {
        Self::move_head(path, expected_revision, false, limits, diagnostics)
    }

    pub fn redo(
        path: &Path,
        expected_revision: &str,
        limits: &ResourceLimits,
        diagnostics: &mut Diagnostics,
    ) -> Result<ProjectChange> {
        Self::move_head(path, expected_revision, true, limits, diagnostics)
    }

    fn move_head(
        path: &Path,
        expected: &str,
        redo: bool,
        limits: &ResourceLimits,
        diagnostics: &mut Diagnostics,
    ) -> Result<ProjectChange> {
        let storage = Storage::open(path)?;
        let _lock = storage.lock()?;
        let project = Self::load(storage, limits, diagnostics)?;
        project.expect(expected)?;
        let boundaries = project
            .history
            .boundaries(&project.manifest.tip_revision.0)?;
        let index = boundaries
            .iter()
            .position(|revision| revision == &project.manifest.current_revision.0)
            .ok_or_else(|| invalid("current revision is outside active history"))?;
        let next = if redo {
            index.checked_add(1)
        } else {
            index.checked_sub(1)
        }
        .and_then(|index| boundaries.get(index))
        .ok_or_else(|| {
            PicError::new(
                ErrorCode::HistoryBoundary,
                if redo {
                    "no active redo group"
                } else {
                    "no group to undo"
                },
            )
        })?;
        // A cursor update is published only if its destination can actually be restored.
        let restored = project.restore(Some(next), diagnostics)?;
        let mut manifest = project.manifest.clone();
        manifest.current_revision = restored.revision.clone();
        let bytes = serialize(&manifest, limits.max_project_bytes)?;
        project.check_metadata_budget(&bytes, 0)?;
        timed(&mut diagnostics.timings.write_ms, || {
            project.publish(&bytes, expected)
        })?;
        Ok(ProjectChange {
            project: project.path().to_owned(),
            previous_revision: Some(project.manifest.current_revision),
            base_revision: None,
            revision: restored.revision,
            commit_id: None,
            steps: Vec::new(),
            published: true,
            width: restored.raster.width(),
            height: restored.raster.height(),
            replay: restored.replay,
        })
    }

    fn expect(&self, expected: &str) -> Result<()> {
        if self.manifest.current_revision.0 != expected {
            return Err(conflict(expected, &self.manifest.current_revision.0));
        }
        Ok(())
    }

    fn check_metadata_budget(&self, manifest_bytes: &[u8], added_commit_bytes: u64) -> Result<()> {
        let existing_commits = self.history.metadata_bytes - self.manifest_bytes.len() as u64;
        let total = existing_commits
            .checked_add(added_commit_bytes)
            .and_then(|bytes| bytes.checked_add(manifest_bytes.len() as u64));
        if total.is_none_or(|bytes| bytes > self.limits.max_project_bytes) {
            return Err(PicError::new(
                ErrorCode::ResourceLimit,
                "project exceeds metadata byte limit",
            ));
        }
        Ok(())
    }

    fn publish(&self, bytes: &[u8], expected: &str) -> Result<()> {
        // Recheck immediately before the only publication point, still holding the writer lock.
        let current = self.storage.manifest(self.limits.max_project_bytes)?;
        let manifest: Manifest = history::parse(&current)?;
        if manifest.current_revision.0 != expected {
            return Err(conflict(expected, &manifest.current_revision.0));
        }
        if current != self.manifest_bytes {
            return Err(PicError::new(
                ErrorCode::RevisionConflict,
                "manifest changed while preparing commit; inspect and retry",
            ));
        }
        self.storage.publish_manifest(bytes, false)
    }

    /// Full-resolution export never reads the preview cache.
    pub fn export(
        &self,
        request: ExportRequest<'_>,
        diagnostics: &mut Diagnostics,
    ) -> Result<ProjectExport> {
        let (output, encoding) = timed(&mut diagnostics.timings.validation_ms, || {
            let encoding = request.encoding.resolve(request.output)?;
            let output = codec::prepare_output(request.output, request.overwrite)?;
            if output.starts_with(self.path()) {
                return Err(PicError::new(
                    ErrorCode::UnsafePath,
                    "export/preview output must be outside the project directory",
                ));
            }
            Ok((output, encoding))
        })?;
        let restored = self.restore(request.revision, diagnostics)?;
        let bytes = timed(&mut diagnostics.timings.encode_ms, || {
            codec::encode(&restored.raster, &encoding, &self.limits)
        })?;
        timed(&mut diagnostics.timings.write_ms, || {
            codec::publish(&output, &bytes, request.overwrite)
        })?;
        diagnostics.warnings.push(Warning {
            code: "metadata_not_preserved",
            message: "Export writes pixels only; source metadata is not preserved.",
        });
        if restored.raster.has_transparency() && encoding.jpeg_background.is_some() {
            diagnostics.warnings.push(Warning { code: "alpha_flattened", message: "JPEG export composites transparency over the explicit background in linear sRGB." });
        }
        Ok(ProjectExport {
            project: self.path().to_owned(),
            target: TargetId::canvas(),
            op_id: self
                .history
                .step(&restored.revision.0)
                .map(|(_, step)| step.op_id.clone()),
            revision: restored.revision,
            output,
            width: restored.raster.width(),
            height: restored.raster.height(),
            operations_replayed: restored.operations_replayed,
            replay: restored.replay,
            format: encoding.format,
            jpeg_quality: encoding.jpeg_quality,
            png_compression: encoding.png_compression,
            jpeg_background: encoding.jpeg_background,
        })
    }
}

fn conflict(expected: &str, current: &str) -> PicError {
    PicError::new(
        ErrorCode::RevisionConflict,
        format!(
            "expected_revision '{expected}' does not match current revision '{current}'; inspect and retry"
        ),
    )
}

fn serialize(value: &impl Serialize, limit: u64) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| invalid(e.to_string()))?;
    if bytes.len() as u64 > limit {
        return Err(PicError::new(
            ErrorCode::ResourceLimit,
            "project metadata exceeds byte limit",
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
