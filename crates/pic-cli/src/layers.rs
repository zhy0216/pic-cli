//! Thin authoring commands: every edit becomes one ordinary immutable operation.
use crate::Data;
use clap::{Args, Subcommand};
use pic_core::{
    ErrorCode, PicError, Result,
    composite::{BlendMode, Transform},
    document::{Region, Selection, TargetId},
    limits::ResourceLimits,
    operation::{
        Operation, OperationSpec,
        geometry::Interpolation,
        layers::{LayerAddParams, LayerOperation, LayerSetParams, MaskParams, ReorderParams},
    },
    pipeline::Pipeline,
    project::{ApplyRequest, Project},
    result::Diagnostics,
};
use std::path::PathBuf;

#[derive(Args)]
pub(super) struct TransformArgs {
    #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
    x: f64,
    #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
    y: f64,
    #[arg(long, default_value_t = 1.0)]
    scale_x: f64,
    #[arg(long, default_value_t = 1.0)]
    scale_y: f64,
    /// Clockwise around local origin, after flips and scale; then apply x/y
    #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
    degrees: f64,
    #[arg(long)]
    flip_x: bool,
    #[arg(long)]
    flip_y: bool,
    #[arg(long, default_value = "bilinear")]
    filter: Interpolation,
}
impl TransformArgs {
    pub fn parameters(self) -> Transform {
        Transform {
            x: self.x,
            y: self.y,
            scale_x: self.scale_x,
            scale_y: self.scale_y,
            degrees: self.degrees,
            flip_x: self.flip_x,
            flip_y: self.flip_y,
            filter: self.filter,
        }
    }
}
#[derive(Args)]
pub(super) struct EditArgs {
    project: PathBuf,
    #[arg(long, visible_alias = "expected-revision")]
    expect_revision: String,
    #[arg(long, visible_alias = "from-revision")]
    revision: Option<String>,
}
impl EditArgs {
    fn apply(
        self,
        spec: OperationSpec,
        limits: &ResourceLimits,
        diagnostics: &mut Diagnostics,
    ) -> Result<Data> {
        let cwd = std::env::current_dir()
            .map_err(|e| PicError::io("read working directory", std::path::Path::new("."), e))?;
        let pipeline = Pipeline::single(spec, &cwd, limits)?;
        Project::apply(
            ApplyRequest {
                project: &self.project,
                pipeline: &pipeline,
                expected_revision: &self.expect_revision,
                revision: self.revision.as_deref(),
            },
            limits,
            diagnostics,
        )
        .map(|value| Data::ProjectChange(Box::new(value)))
    }
    fn layer(
        self,
        target: String,
        operation: LayerOperation,
        limits: &ResourceLimits,
        diagnostics: &mut Diagnostics,
    ) -> Result<Data> {
        let mut spec = Operation::Layer(operation).to_spec();
        spec.target = TargetId(target);
        self.apply(spec, limits, diagnostics)
    }
}
#[derive(Subcommand)]
pub(super) enum LayerCommand {
    /// Add an independent image above existing layers; caller supplies a new stable ID
    Add {
        #[command(flatten)]
        edit: EditArgs,
        #[arg(long)]
        id: String,
        #[arg(long)]
        source: PathBuf,
        #[arg(long, default_value = "Layer")]
        name: String,
    },
    /// Set only the supplied display/compositing properties
    Set {
        #[command(flatten)]
        edit: EditArgs,
        #[arg(long)]
        target: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        visible: Option<bool>,
        #[arg(long)]
        opacity: Option<f64>,
        #[arg(long)]
        blend: Option<BlendMode>,
    },
    /// Replace the complete non-destructive transform; omitted fields use identity defaults
    Transform {
        #[command(flatten)]
        edit: EditArgs,
        #[arg(long)]
        target: String,
        #[command(flatten)]
        transform: TransformArgs,
    },
    /// Move immediately below --before ID, or to the top when omitted
    Reorder {
        #[command(flatten)]
        edit: EditArgs,
        #[arg(long)]
        target: String,
        #[arg(long)]
        before: Option<String>,
    },
    /// Remove the named ID; never reuses that ID along this history
    Remove {
        #[command(flatten)]
        edit: EditArgs,
        #[arg(long)]
        target: String,
    },
    /// Execute an existing pixel operation against local pixels of a stable layer ID
    Edit {
        #[command(flatten)]
        edit: EditArgs,
        #[arg(long)]
        target: String,
        #[arg(long)]
        op: String,
        #[arg(long, default_value = "{}")]
        params: String,
    },
}
impl LayerCommand {
    pub fn execute(self, limits: &ResourceLimits, diagnostics: &mut Diagnostics) -> Result<Data> {
        match self {
            Self::Add {
                edit,
                id,
                source,
                name,
            } => edit.layer(
                "canvas".into(),
                LayerOperation::Add(LayerAddParams {
                    id: TargetId(id),
                    source: path_string(source)?,
                    name,
                }),
                limits,
                diagnostics,
            ),
            Self::Set {
                edit,
                target,
                name,
                visible,
                opacity,
                blend,
            } => edit.layer(
                target,
                LayerOperation::Set(LayerSetParams {
                    name,
                    visible,
                    opacity,
                    blend,
                }),
                limits,
                diagnostics,
            ),
            Self::Transform {
                edit,
                target,
                transform,
            } => edit.layer(
                target,
                LayerOperation::Transform(transform.parameters()),
                limits,
                diagnostics,
            ),
            Self::Reorder {
                edit,
                target,
                before,
            } => edit.layer(
                target,
                LayerOperation::Reorder(ReorderParams {
                    before: before.map(TargetId),
                }),
                limits,
                diagnostics,
            ),
            Self::Remove { edit, target } => {
                edit.layer(target, LayerOperation::Remove, limits, diagnostics)
            }
            Self::Edit {
                edit,
                target,
                op,
                params,
            } => {
                let params = serde_json::from_str(&params)
                    .map_err(|e| PicError::new(ErrorCode::InvalidJson, e.to_string()))?;
                edit.apply(
                    OperationSpec {
                        op,
                        op_version: 1,
                        target: TargetId(target),
                        params,
                    },
                    limits,
                    diagnostics,
                )
            }
        }
    }
}
#[derive(Subcommand)]
pub(super) enum MaskCommand {
    /// Import local-size encoded grayscale coverage, multiplied by its alpha
    Set {
        #[command(flatten)]
        edit: EditArgs,
        #[arg(long)]
        target: String,
        #[arg(long)]
        source: PathBuf,
    },
    Remove {
        #[command(flatten)]
        edit: EditArgs,
        #[arg(long)]
        target: String,
    },
}
impl MaskCommand {
    pub fn execute(self, limits: &ResourceLimits, diagnostics: &mut Diagnostics) -> Result<Data> {
        match self {
            Self::Set {
                edit,
                target,
                source,
            } => edit.layer(
                target,
                LayerOperation::MaskSet(MaskParams {
                    source: path_string(source)?,
                }),
                limits,
                diagnostics,
            ),
            Self::Remove { edit, target } => {
                edit.layer(target, LayerOperation::MaskRemove, limits, diagnostics)
            }
        }
    }
}
#[derive(Subcommand)]
pub(super) enum SelectionCommand {
    /// Replace selection with a union of rectangles in canvas or named layer coordinates
    Set {
        #[command(flatten)]
        edit: EditArgs,
        #[arg(long, default_value = "canvas")]
        space: String,
        /// Repeat for a union; half-open x,y,width,height in continuous pixel-edge coordinates
        #[arg(long,value_parser=parse_region,required=true)]
        region: Vec<Region>,
    },
    /// Remove the selection restriction from subsequent local pixel edits
    Clear {
        #[command(flatten)]
        edit: EditArgs,
    },
}
impl SelectionCommand {
    pub fn execute(self, limits: &ResourceLimits, diagnostics: &mut Diagnostics) -> Result<Data> {
        match self {
            Self::Set {
                edit,
                space,
                region,
            } => edit.layer(
                "canvas".into(),
                LayerOperation::SelectionSet(Selection {
                    space: TargetId(space),
                    regions: region,
                }),
                limits,
                diagnostics,
            ),
            Self::Clear { edit } => edit.layer(
                "canvas".into(),
                LayerOperation::SelectionClear,
                limits,
                diagnostics,
            ),
        }
    }
}
fn parse_region(value: &str) -> std::result::Result<Region, String> {
    let values = value
        .split(',')
        .map(str::parse::<f64>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| "region requires x,y,width,height".to_owned())?;
    let [x, y, width, height] = values.as_slice() else {
        return Err("region requires x,y,width,height".into());
    };
    Ok(Region {
        x: *x,
        y: *y,
        width: *width,
        height: *height,
    })
}
pub(super) fn path_string(path: PathBuf) -> Result<String> {
    path.into_os_string()
        .into_string()
        .map_err(|_| PicError::new(ErrorCode::InvalidArgument, "resource path must be UTF-8"))
}
