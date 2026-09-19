use std::{
    ffi::OsString,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    time::Instant,
};

use clap::{Args, Parser, Subcommand, error::ErrorKind};
use pic_core::{
    ErrorCode, PicError, Result,
    capabilities::{self, Capabilities},
    codec::{self, EncodeOptions, Format, ImageInfo},
    limits::ResourceLimits,
    operation::{
        Operation,
        adjustments::{
            AdjustParams, BlurParams, Channel, CurvesParams, LevelsParams, SharpenParams,
        },
        geometry::{
            Anchor, CanvasParams, CropParams, FlipAxis, FlipParams, Interpolation, ResizeParams,
            RgbaColor, RotateParams,
        },
    },
    pipeline::{self, Pipeline, RunRequest, RunResult},
    result::{Diagnostics, Envelope, timed},
};
use serde::Serialize;

mod layers;
mod project;
mod typography;

#[derive(Parser)]
#[command(
    version,
    about = "Explicit image processing for agents",
    long_about = "Explicit image processing for agents. PNG/JPEG codecs, geometry, color adjustments, filters and editable layers/masks in linear sRGB. Stable IDs, explicit selections and immutable project history. Ordered RGBA32F operations; quantization only at export. Coordinates use the current canvas after EXIF normalization. Query capabilities for equations, ranges and alpha behavior."
)]
struct Cli {
    /// Emit one compact, versioned JSON result (including errors/help/version)
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Self-contained .pic projects, immutable ops, revisions and undo/redo
    Project {
        #[command(flatten)]
        cache: project::CacheArgs,
        #[command(subcommand)]
        command: project::ProjectCommand,
    },
    /// Show the executable version
    Version,
    /// Report supported, partial and unimplemented capabilities
    Capabilities,
    /// Decode and inspect an accepted PNG/JPEG image
    Info { input: PathBuf },
    /// Execute a schema-v1 pipeline in array order on the current canvas
    Run {
        #[command(flatten)]
        image: ImageArgs,
        /// JSON file; resource paths are relative to this file's directory
        #[arg(long)]
        pipeline: PathBuf,
    },
    /// Decode and re-encode via the same core as a one-step identity pipeline
    Identity(ImageArgs),
    /// Composite an external image using the same linear-light renderer as project layers
    Composite {
        #[command(flatten)]
        image: ImageArgs,
        #[arg(long)]
        overlay: PathBuf,
        #[arg(long)]
        mask: Option<PathBuf>,
        #[arg(long, default_value_t = 1.0)]
        opacity: f64,
        #[arg(long, default_value = "normal")]
        blend: pic_core::composite::BlendMode,
        #[command(flatten)]
        transform: layers::TransformArgs,
    },
    /// Extract an in-bounds rectangle; origin top-left, x right, y down
    Crop {
        #[command(flatten)]
        image: ImageArgs,
        #[arg(long)]
        x: u32,
        #[arg(long)]
        y: u32,
        #[arg(long)]
        width: u32,
        #[arg(long)]
        height: u32,
    },
    /// Resize; one dimension preserves aspect ratio, both set the exact size
    Resize {
        #[command(flatten)]
        image: ImageArgs,
        #[arg(long)]
        width: Option<u32>,
        #[arg(long)]
        height: Option<u32>,
        /// nearest or bilinear (antialiased downsampling in linear light)
        #[arg(long, default_value = "bilinear")]
        filter: Interpolation,
    },
    /// Rotate clockwise about the canvas center, expanding bounds by default
    Rotate {
        #[command(flatten)]
        image: ImageArgs,
        /// Finite clockwise degrees in [-360, 360]; right angles copy exact pixels
        #[arg(long, allow_hyphen_values = true)]
        degrees: f64,
        /// Keep current dimensions; clip rotated content at the canvas bounds
        #[arg(long)]
        keep_size: bool,
        /// nearest or bilinear; non-square keep-size quarter turns also resample
        #[arg(long, default_value = "bilinear")]
        filter: Interpolation,
        /// Outside samples: sRGB #RRGGBB or #RRGGBBAA
        #[arg(long, default_value = "#00000000")]
        background: RgbaColor,
    },
    /// Mirror the current canvas horizontally or vertically
    Flip {
        #[command(flatten)]
        image: ImageArgs,
        /// horizontal or vertical
        #[arg(long)]
        axis: FlipAxis,
    },
    /// Change canvas dimensions by padding or clipping, without scaling
    Canvas {
        #[command(flatten)]
        image: ImageArgs,
        #[arg(long)]
        width: u32,
        #[arg(long)]
        height: u32,
        /// top_left, top, top_right, left, center, right, bottom_left, bottom, bottom_right
        #[arg(long, default_value = "center")]
        anchor: Anchor,
        /// Padding only (existing alpha stays intact): sRGB #RRGGBB or #RRGGBBAA
        #[arg(long, default_value = "#00000000")]
        background: RgbaColor,
    },
    /// Apply exposure, brightness, contrast, then saturation in linear RGB; preserve alpha
    Adjust {
        #[command(flatten)]
        image: ImageArgs,
        /// EV [-20,20]: multiply RGB by 2^EV
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        exposure: f64,
        /// Linear RGB offset [-1,1]
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        brightness: f64,
        /// Multiplier [0,10] about linear RGB 0.5; 1 is identity
        #[arg(long, default_value_t = 1.0, allow_hyphen_values = true)]
        contrast: f64,
        /// Multiplier [0,10] about Rec.709 luminance; 0 gray, 1 identity
        #[arg(long, default_value_t = 1.0, allow_hyphen_values = true)]
        saturation: f64,
    },
    /// Map linear RGB black/white points and signed gamma, without clipping; preserve alpha
    Levels {
        #[command(flatten)]
        image: ImageArgs,
        /// rgb, red, green or blue; never alpha
        #[arg(long, default_value = "rgb")]
        channel: Channel,
        /// [0,1], strictly below input-white
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        input_black: f64,
        /// [0,1], strictly above input-black
        #[arg(long, default_value_t = 1.0, allow_hyphen_values = true)]
        input_white: f64,
        /// [0.1,10]: signed power 1/gamma of normalized input; 1 identity
        #[arg(long, default_value_t = 1.0, allow_hyphen_values = true)]
        gamma: f64,
        /// [0,1], at most output-white
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        output_black: f64,
        /// [0,1], at least output-black
        #[arg(long, default_value_t = 1.0, allow_hyphen_values = true)]
        output_white: f64,
    },
    /// Piecewise linear RGB curve; extrapolate end segments outside [0,1]; preserve alpha
    Curves {
        #[command(flatten)]
        image: ImageArgs,
        /// rgb, red, green or blue; never alpha
        #[arg(long, default_value = "rgb")]
        channel: Channel,
        /// JSON array of 2..256 [x,y] pairs in [0,1]; x strictly increases from 0 to 1
        #[arg(long, default_value = "[[0,0],[1,1]]")]
        points: String,
    },
    /// Rec.709 linear luminance (0.2126 R + 0.7152 G + 0.0722 B); preserve alpha
    Grayscale(ImageArgs),
    /// Linear RGB inversion C'=1-C, without clipping; preserve alpha
    Invert(ImageArgs),
    /// Gaussian blur in premultiplied linear RGB/alpha; clamp edge taps, radius ceil(3*sigma)
    Blur {
        #[command(flatten)]
        image: ImageArgs,
        /// Standard deviation [0,100] pixels; 0 exact identity; zero-alpha output RGB is 0
        #[arg(long, default_value_t = 1.0, allow_hyphen_values = true)]
        sigma: f64,
    },
    /// Unsharp mask C'=C+amount*(C-blur(C)); preserve alpha and fully transparent hidden RGB
    Sharpen {
        #[command(flatten)]
        image: ImageArgs,
        /// Gaussian sigma [0,100] pixels, radius ceil(3*sigma), clamp edges; 0 identity
        #[arg(long, default_value_t = 1.0, allow_hyphen_values = true)]
        sigma: f64,
        /// Strength [0,10]; 0 identity; RGB overshoot retained until export
        #[arg(long, default_value_t = 1.0, allow_hyphen_values = true)]
        amount: f64,
    },
}

#[derive(Args)]
struct ImageArgs {
    #[arg(long)]
    input: PathBuf,
    #[command(flatten)]
    output: OutputArgs,
}

#[derive(Args)]
struct OutputArgs {
    #[arg(long)]
    output: PathBuf,
    /// Output format: png or jpeg (jpg); defaults to the output extension
    #[arg(long)]
    format: Option<Format>,
    /// JPEG quality, 1..100 (default 90); invalid for PNG
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=100))]
    jpeg_quality: Option<u8>,
    /// PNG deflate level 0..9 (default 6), adaptive row filter; invalid for JPEG
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=9))]
    png_compression: Option<u8>,
    /// Flatten JPEG alpha over this opaque #RRGGBB color in linear sRGB
    #[arg(long)]
    jpeg_background: Option<RgbaColor>,
    /// Atomically replace an existing regular output file
    #[arg(long)]
    overwrite: bool,
}

impl OutputArgs {
    fn encoding(&self) -> EncodeOptions {
        EncodeOptions {
            format: self.format,
            jpeg_quality: self.jpeg_quality,
            png_compression: self.png_compression,
            jpeg_background: self.jpeg_background,
        }
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum Data {
    Version { version: &'static str },
    Help { help: String },
    Capabilities(Box<Capabilities>),
    Info(ImageInfo),
    Run(RunResult),
    ProjectChange(Box<pic_core::project::ProjectChange>),
    ProjectInspection(Box<pic_core::project::ProjectInspection>),
    ProjectExport(Box<pic_core::project::ProjectExport>),
    ProjectPreview(Box<pic_core::project::ProjectPreview>),
    Checkpoint(Box<pic_core::project::CheckpointResult>),
    CacheClear(pic_core::project::CacheClearResult),
    TemplateExport(Box<pic_core::project::TemplateExport>),
}

impl Command {
    fn name(&self) -> &'static str {
        match self {
            Self::Project { command, .. } => command.name(),
            Self::Version => "version",
            Self::Capabilities => "capabilities",
            Self::Info { .. } => "info",
            Self::Run { .. } => "run",
            Self::Identity(_) => "identity",
            Self::Composite { .. } => "composite",
            Self::Crop { .. } => "crop",
            Self::Resize { .. } => "resize",
            Self::Rotate { .. } => "rotate",
            Self::Flip { .. } => "flip",
            Self::Canvas { .. } => "canvas",
            Self::Adjust { .. } => "adjust",
            Self::Levels { .. } => "levels",
            Self::Curves { .. } => "curves",
            Self::Grayscale(_) => "grayscale",
            Self::Invert(_) => "invert",
            Self::Blur { .. } => "blur",
            Self::Sharpen { .. } => "sharpen",
        }
    }

    fn execute(self, diagnostics: &mut Diagnostics) -> Result<Data> {
        let limits = ResourceLimits::default();
        match self {
            Self::Project { command, cache } => command.execute(&cache.limits(limits), diagnostics),
            Self::Version => Ok(Data::Version {
                version: env!("CARGO_PKG_VERSION"),
            }),
            Self::Capabilities => Ok(Data::Capabilities(Box::new(capabilities::capabilities()))),
            Self::Info { input } => Ok(Data::Info(codec::load(&input, &limits, diagnostics)?.info)),
            Self::Run { image, pipeline } => {
                let pipeline = timed(&mut diagnostics.timings.validation_ms, || {
                    Pipeline::from_file(&pipeline, &limits)
                })?;
                execute_image(image, pipeline, &limits, diagnostics)
            }
            Self::Composite {
                image,
                overlay,
                mask,
                opacity,
                blend,
                transform,
            } => execute_operation(
                image,
                Operation::Composite(pic_core::operation::layers::CompositeParams {
                    source: layers::path_string(overlay)?,
                    mask: mask.map(layers::path_string).transpose()?,
                    opacity,
                    blend,
                    transform: transform.parameters(),
                }),
                &limits,
                diagnostics,
            ),
            Self::Identity(image) => {
                execute_operation(image, Operation::Identity, &limits, diagnostics)
            }
            Self::Crop {
                image,
                x,
                y,
                width,
                height,
            } => execute_operation(
                image,
                Operation::Crop(CropParams {
                    x,
                    y,
                    width,
                    height,
                }),
                &limits,
                diagnostics,
            ),
            Self::Resize {
                image,
                width,
                height,
                filter,
            } => execute_operation(
                image,
                Operation::Resize(ResizeParams {
                    width,
                    height,
                    filter,
                }),
                &limits,
                diagnostics,
            ),
            Self::Rotate {
                image,
                degrees,
                keep_size,
                filter,
                background,
            } => execute_operation(
                image,
                Operation::Rotate(RotateParams {
                    degrees,
                    expand: !keep_size,
                    filter,
                    background,
                }),
                &limits,
                diagnostics,
            ),
            Self::Flip { image, axis } => execute_operation(
                image,
                Operation::Flip(FlipParams { axis }),
                &limits,
                diagnostics,
            ),
            Self::Canvas {
                image,
                width,
                height,
                anchor,
                background,
            } => execute_operation(
                image,
                Operation::Canvas(CanvasParams {
                    width,
                    height,
                    anchor,
                    background,
                }),
                &limits,
                diagnostics,
            ),
            Self::Adjust {
                image,
                exposure,
                brightness,
                contrast,
                saturation,
            } => execute_operation(
                image,
                Operation::Adjust(AdjustParams {
                    exposure,
                    brightness,
                    contrast,
                    saturation,
                }),
                &limits,
                diagnostics,
            ),
            Self::Levels {
                image,
                channel,
                input_black,
                input_white,
                gamma,
                output_black,
                output_white,
            } => execute_operation(
                image,
                Operation::Levels(LevelsParams {
                    channel,
                    input_black,
                    input_white,
                    gamma,
                    output_black,
                    output_white,
                }),
                &limits,
                diagnostics,
            ),
            Self::Curves {
                image,
                channel,
                points,
            } => {
                let points = serde_json::from_str(&points).map_err(|e| {
                    PicError::new(ErrorCode::InvalidArgument, format!("curve points: {e}"))
                })?;
                execute_operation(
                    image,
                    Operation::Curves(CurvesParams { channel, points }),
                    &limits,
                    diagnostics,
                )
            }
            Self::Grayscale(image) => {
                execute_operation(image, Operation::Grayscale, &limits, diagnostics)
            }
            Self::Invert(image) => {
                execute_operation(image, Operation::Invert, &limits, diagnostics)
            }
            Self::Blur { image, sigma } => execute_operation(
                image,
                Operation::Blur(BlurParams { sigma }),
                &limits,
                diagnostics,
            ),
            Self::Sharpen {
                image,
                sigma,
                amount,
            } => execute_operation(
                image,
                Operation::Sharpen(SharpenParams { sigma, amount }),
                &limits,
                diagnostics,
            ),
        }
    }
}

fn execute_operation(
    image: ImageArgs,
    operation: Operation,
    limits: &ResourceLimits,
    diagnostics: &mut Diagnostics,
) -> Result<Data> {
    let pipeline = timed(&mut diagnostics.timings.validation_ms, || {
        operation.validate()?;
        let cwd = std::env::current_dir()
            .map_err(|e| PicError::io("read working directory", std::path::Path::new("."), e))?;
        Pipeline::single(operation.to_spec(), &cwd, limits)
    })?;
    execute_image(image, pipeline, limits, diagnostics)
}

fn execute_image(
    image: ImageArgs,
    pipeline: Pipeline,
    limits: &ResourceLimits,
    diagnostics: &mut Diagnostics,
) -> Result<Data> {
    pipeline::run(
        RunRequest {
            input: &image.input,
            output: &image.output.output,
            pipeline: &pipeline,
            encoding: image.output.encoding(),
            overwrite: image.output.overwrite,
        },
        limits,
        diagnostics,
    )
    .map(Data::Run)
}

fn emit(
    command: &str,
    result: Result<Data>,
    mut diagnostics: Diagnostics,
    json: bool,
    start: Instant,
    error_exit: u8,
) -> ExitCode {
    diagnostics.timings.total_ms = start.elapsed().as_secs_f64() * 1000.0;
    let envelope = Envelope::new(command, result, diagnostics);
    let mut stdout = io::stdout().lock();
    let written = match (&envelope.data, json) {
        (Some(Data::Help { help }), false) => stdout.write_all(help.as_bytes()),
        (Some(Data::Version { version }), false) => writeln!(stdout, "pic-cli {version}"),
        _ => {
            let serialized = if json {
                serde_json::to_vec(&envelope)
            } else {
                serde_json::to_vec_pretty(&envelope)
            };
            serialized.map_err(io::Error::other).and_then(|bytes| {
                stdout.write_all(&bytes)?;
                stdout.write_all(b"\n")
            })
        }
    }
    .and_then(|()| stdout.flush());
    if let Err(error) = written {
        eprintln!("cannot write result: {error}");
        return ExitCode::FAILURE;
    }
    if !json && let Some(error) = &envelope.error {
        eprintln!("{error}");
    }
    if envelope.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(error_exit)
    }
}

fn main() -> ExitCode {
    let start = Instant::now();
    let args: Vec<OsString> = std::env::args_os().collect();
    // Clap errors and early help/version exits must honor JSON too.
    let json = args
        .iter()
        .skip(1)
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--json");
    let mut diagnostics = Diagnostics::default();
    match Cli::try_parse_from(&args) {
        Ok(cli) => {
            let command = cli.command.name();
            let result = cli.command.execute(&mut diagnostics);
            emit(command, result, diagnostics, cli.json, start, 1)
        }
        Err(error) => {
            let (command, result) = match error.kind() {
                ErrorKind::DisplayHelp => (
                    "help",
                    Ok(Data::Help {
                        help: error.to_string(),
                    }),
                ),
                ErrorKind::DisplayVersion => (
                    "version",
                    Ok(Data::Version {
                        version: env!("CARGO_PKG_VERSION"),
                    }),
                ),
                _ => (
                    "cli",
                    Err(PicError::new(ErrorCode::InvalidArgument, error.to_string())),
                ),
            };
            emit(command, result, diagnostics, json, start, 2)
        }
    }
}
