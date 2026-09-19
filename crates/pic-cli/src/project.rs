use std::path::PathBuf;

use clap::{Args, Subcommand};
use pic_core::{
    ErrorCode, PicError, Result,
    document::TargetId,
    limits::ResourceLimits,
    operation::geometry::{CropParams, Interpolation},
    pipeline::Pipeline,
    project::{
        ApplyRequest, ExportRequest, PreviewRequest, Project, ReviseRequest, TemplateRunRequest,
    },
    result::{Diagnostics, timed},
};

use crate::{Data, OutputArgs};

#[derive(Subcommand)]
pub(super) enum ProjectCommand {
    /// Create isolated groups; use layer parent/set/transform/reorder for group editing
    Group {
        #[command(subcommand)]
        command: crate::typography::GroupCommand,
    },
    /// Editable text with an embedded explicit font, LTR shaping, LF breaks and alignment
    Text {
        #[command(subcommand)]
        command: crate::typography::TextCommand,
    },
    /// Non-destructive point adjustment of lower siblings in the same isolated scope
    Adjustment {
        #[command(subcommand)]
        command: crate::typography::AdjustmentCommand,
    },
    /// Edit independent layers using stable IDs; all mutations require expected revision
    Layer {
        #[command(subcommand)]
        command: crate::layers::LayerCommand,
    },
    /// Attach or remove an external grayscale coverage mask on a stable layer
    Mask {
        #[command(subcommand)]
        command: crate::layers::MaskCommand,
    },
    /// Set or clear a rectangular union used by subsequent layer pixel edits
    Selection {
        #[command(subcommand)]
        command: crate::layers::SelectionCommand,
    },
    /// Import original PNG/JPEG bytes into a new .pic directory; starts at r0
    Create {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Execute and atomically commit a nonempty pipeline as one undo group
    Apply {
        project: PathBuf,
        #[arg(long)]
        pipeline: PathBuf,
        /// Must equal the current pointer, also when --revision selects an old base
        #[arg(long, visible_alias = "expected-revision")]
        expect_revision: String,
        /// Continue from any committed step; clears active redo, retains old records
        #[arg(long, visible_alias = "from-revision")]
        revision: Option<String>,
    },
    /// Restore and inspect a step, all immutable records and active group boundaries
    Inspect {
        project: PathBuf,
        #[arg(long)]
        revision: Option<String>,
    },
    /// Restore a committed revision and export with the ordinary codec options
    Export(RenderArgs),
    /// Preview any committed step, canvas region and output size, with coordinate mappings
    Preview(PreviewArgs),
    /// Store a disposable, exact RGBA32F checkpoint of a committed revision
    Checkpoint {
        project: PathBuf,
        #[arg(long)]
        revision: Option<String>,
    },
    /// Remove all managed checkpoints and preview caches; retain assets and history
    CacheClear { project: PathBuf },
    /// Replace a current-history step's full parameters and commit a new suffix
    Revise {
        project: PathBuf,
        #[arg(long)]
        step_revision: String,
        /// Complete JSON params object for the same operation type
        #[arg(long)]
        params: String,
        #[arg(long, visible_alias = "expected-revision")]
        expect_revision: String,
    },
    /// Export input/target/parameter slots; suggestions never replace required bindings
    TemplateExport {
        project: PathBuf,
        #[arg(long)]
        revision: Option<String>,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        overwrite: bool,
    },
    /// Bind a template to new input and create a new self-contained .pic history
    TemplateRun {
        #[arg(long)]
        template: PathBuf,
        #[arg(long)]
        bindings: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Restore the previous whole group boundary; append no operations
    Undo(CursorArgs),
    /// Restore the next active group boundary; abandoned redo paths stay read-only
    Redo(CursorArgs),
}

#[derive(Args)]
pub(super) struct CacheArgs {
    /// Combined disk budget for derived cache files, in bytes (default 536870912)
    #[arg(long, global = true)]
    cache_disk_bytes: Option<u64>,
    /// Memory cache/snapshot scratch budget in bytes (default 268435456)
    #[arg(long, global = true)]
    cache_memory_bytes: Option<u64>,
}

impl CacheArgs {
    pub fn limits(&self, mut limits: ResourceLimits) -> ResourceLimits {
        if let Some(value) = self.cache_disk_bytes {
            limits.max_cache_disk_bytes = value;
        }
        if let Some(value) = self.cache_memory_bytes {
            limits.max_cache_memory_bytes = value;
        }
        limits
    }
}

#[derive(Args)]
pub(super) struct PreviewArgs {
    #[command(flatten)]
    render: RenderArgs,
    /// canvas, a stable layer ID, or mask:<layer ID>; output always uses canvas coordinates
    #[arg(long, default_value = "canvas")]
    target: String,
    /// Half-open canvas rectangle x,y,width,height at the selected revision
    #[arg(long, value_parser = parse_region)]
    region: Option<CropParams>,
    /// One dimension preserves the region aspect ratio; two set an exact size
    #[arg(long)]
    width: Option<u32>,
    #[arg(long)]
    height: Option<u32>,
    #[arg(long, default_value = "bilinear")]
    filter: Interpolation,
}

fn parse_region(value: &str) -> std::result::Result<CropParams, String> {
    let parts: Vec<u32> = value
        .split(',')
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| "region requires x,y,width,height as unsigned integers".to_owned())?;
    let [x, y, width, height] = parts.as_slice() else {
        return Err("region requires x,y,width,height".into());
    };
    let region = CropParams {
        x: *x,
        y: *y,
        width: *width,
        height: *height,
    };
    region.validate().map_err(|e| e.to_string())?;
    Ok(region)
}

#[derive(Args)]
pub(super) struct RenderArgs {
    project: PathBuf,
    #[arg(long)]
    revision: Option<String>,
    #[command(flatten)]
    output: OutputArgs,
}

#[derive(Args)]
pub(super) struct CursorArgs {
    project: PathBuf,
    #[arg(long, visible_alias = "expected-revision")]
    expect_revision: String,
}

impl ProjectCommand {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Layer { .. } => "project layer",
            Self::Group { .. } => "project group",
            Self::Text { .. } => "project text",
            Self::Adjustment { .. } => "project adjustment",
            Self::Mask { .. } => "project mask",
            Self::Selection { .. } => "project selection",
            Self::Create { .. } => "project create",
            Self::Apply { .. } => "project apply",
            Self::Inspect { .. } => "project inspect",
            Self::Export(_) => "project export",
            Self::Preview(_) => "project preview",
            Self::Checkpoint { .. } => "project checkpoint",
            Self::CacheClear { .. } => "project cache-clear",
            Self::Revise { .. } => "project revise",
            Self::TemplateExport { .. } => "project template-export",
            Self::TemplateRun { .. } => "project template-run",
            Self::Undo(_) => "project undo",
            Self::Redo(_) => "project redo",
        }
    }

    pub fn execute(self, limits: &ResourceLimits, diagnostics: &mut Diagnostics) -> Result<Data> {
        match self {
            Self::Layer { command } => command.execute(limits, diagnostics),
            Self::Group { command } => command.execute(limits, diagnostics),
            Self::Text { command } => command.execute(limits, diagnostics),
            Self::Adjustment { command } => command.execute(limits, diagnostics),
            Self::Mask { command } => command.execute(limits, diagnostics),
            Self::Selection { command } => command.execute(limits, diagnostics),
            Self::Create { input, output } => Project::create(&input, &output, limits, diagnostics)
                .map(|value| Data::ProjectChange(Box::new(value))),
            Self::Apply {
                project,
                pipeline,
                expect_revision,
                revision,
            } => {
                let pipeline = timed(&mut diagnostics.timings.validation_ms, || {
                    Pipeline::from_file(&pipeline, limits)
                })?;
                Project::apply(
                    ApplyRequest {
                        project: &project,
                        pipeline: &pipeline,
                        expected_revision: &expect_revision,
                        revision: revision.as_deref(),
                    },
                    limits,
                    diagnostics,
                )
                .map(|value| Data::ProjectChange(Box::new(value)))
            }
            Self::Inspect { project, revision } => Project::open(&project, limits, diagnostics)?
                .inspect(revision.as_deref(), diagnostics)
                .map(|value| Data::ProjectInspection(Box::new(value))),
            Self::Export(args) => Project::open(&args.project, limits, diagnostics)?
                .export(
                    ExportRequest {
                        revision: args.revision.as_deref(),
                        output: &args.output.output,
                        encoding: args.output.encoding(),
                        overwrite: args.output.overwrite,
                    },
                    diagnostics,
                )
                .map(|value| Data::ProjectExport(Box::new(value))),
            Self::Preview(args) => {
                let render = args.render;
                Project::open(&render.project, limits, diagnostics)?
                    .preview(
                        PreviewRequest {
                            export: ExportRequest {
                                revision: render.revision.as_deref(),
                                output: &render.output.output,
                                encoding: render.output.encoding(),
                                overwrite: render.output.overwrite,
                            },
                            target: TargetId(args.target),
                            region: args.region,
                            width: args.width,
                            height: args.height,
                            filter: args.filter,
                        },
                        diagnostics,
                    )
                    .map(|value| Data::ProjectPreview(Box::new(value)))
            }
            Self::Checkpoint { project, revision } => Project::open(&project, limits, diagnostics)?
                .checkpoint(revision.as_deref(), diagnostics)
                .map(|value| Data::Checkpoint(Box::new(value))),
            Self::CacheClear { project } => Project::open(&project, limits, diagnostics)?
                .clear_cache(diagnostics)
                .map(Data::CacheClear),
            Self::Revise {
                project,
                step_revision,
                params,
                expect_revision,
            } => {
                let params = serde_json::from_str(&params)
                    .map_err(|e| PicError::new(ErrorCode::InvalidJson, e.to_string()))?;
                Project::revise(
                    ReviseRequest {
                        project: &project,
                        step_revision: &step_revision,
                        params,
                        expected_revision: &expect_revision,
                    },
                    limits,
                    diagnostics,
                )
                .map(|value| Data::ProjectChange(Box::new(value)))
            }
            Self::TemplateExport {
                project,
                revision,
                output,
                overwrite,
            } => Project::open(&project, limits, diagnostics)?
                .export_template(revision.as_deref(), &output, overwrite, diagnostics)
                .map(|value| Data::TemplateExport(Box::new(value))),
            Self::TemplateRun {
                template,
                bindings,
                output,
            } => Project::run_template(
                TemplateRunRequest {
                    template: &template,
                    bindings: &bindings,
                    output: &output,
                },
                limits,
                diagnostics,
            )
            .map(|value| Data::ProjectChange(Box::new(value))),
            Self::Undo(args) => {
                Project::undo(&args.project, &args.expect_revision, limits, diagnostics)
                    .map(|value| Data::ProjectChange(Box::new(value)))
            }
            Self::Redo(args) => {
                Project::redo(&args.project, &args.expect_revision, limits, diagnostics)
                    .map(|value| Data::ProjectChange(Box::new(value)))
            }
        }
    }
}
