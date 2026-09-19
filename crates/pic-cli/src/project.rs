use std::path::PathBuf;

use clap::{Args, Subcommand};
use pic_core::{
    Result,
    limits::ResourceLimits,
    pipeline::Pipeline,
    project::{ApplyRequest, ExportRequest, Project},
    result::{Diagnostics, timed},
};

use crate::{Data, OutputArgs};

#[derive(Subcommand)]
pub(super) enum ProjectCommand {
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
    /// Read-only full-resolution step preview; region/scaled previews are planned
    Preview(RenderArgs),
    /// Restore the previous whole group boundary; append no operations
    Undo(CursorArgs),
    /// Restore the next active group boundary; abandoned redo paths stay read-only
    Redo(CursorArgs),
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
            Self::Create { .. } => "project create",
            Self::Apply { .. } => "project apply",
            Self::Inspect { .. } => "project inspect",
            Self::Export(_) => "project export",
            Self::Preview(_) => "project preview",
            Self::Undo(_) => "project undo",
            Self::Redo(_) => "project redo",
        }
    }

    pub fn execute(self, limits: &ResourceLimits, diagnostics: &mut Diagnostics) -> Result<Data> {
        match self {
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
            Self::Export(args) | Self::Preview(args) => {
                Project::open(&args.project, limits, diagnostics)?
                    .export(
                        ExportRequest {
                            revision: args.revision.as_deref(),
                            output: &args.output.output,
                            encoding: args.output.encoding(),
                            overwrite: args.output.overwrite,
                        },
                        diagnostics,
                    )
                    .map(|value| Data::ProjectExport(Box::new(value)))
            }
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
