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
    operation::OperationSpec,
    pipeline::{self, Pipeline, RunRequest, RunResult},
    result::{Diagnostics, Envelope, timed},
};
use serde::Serialize;

#[derive(Parser)]
#[command(
    version,
    about = "Explicit image processing for agents",
    long_about = "Explicit image processing for agents. Foundation release: PNG/JPEG info, empty pipelines and identity only. Query capabilities for exact support."
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
    /// Show the executable version
    Version,
    /// Report supported, partial and unimplemented capabilities
    Capabilities,
    /// Decode and inspect an accepted PNG/JPEG image
    Info { input: PathBuf },
    /// Execute a schema-v1 pipeline (empty or identity operations)
    Run {
        #[command(flatten)]
        image: ImageArgs,
        /// JSON file; resource paths are relative to this file's directory
        #[arg(long)]
        pipeline: PathBuf,
    },
    /// Decode and re-encode via the same core as a one-step identity pipeline
    Identity(ImageArgs),
}

#[derive(Args)]
struct ImageArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    output: PathBuf,
    /// Output format: png or jpeg (jpg); defaults to the output extension
    #[arg(long)]
    format: Option<Format>,
    /// JPEG quality, 1..100 (default 90); invalid for PNG
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=100))]
    jpeg_quality: Option<u8>,
    /// Atomically replace an existing regular output file
    #[arg(long)]
    overwrite: bool,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Data {
    Version { version: &'static str },
    Help { help: String },
    Capabilities(Capabilities),
    Info(ImageInfo),
    Run(RunResult),
}

impl Command {
    fn name(&self) -> &'static str {
        match self {
            Self::Version => "version",
            Self::Capabilities => "capabilities",
            Self::Info { .. } => "info",
            Self::Run { .. } => "run",
            Self::Identity(_) => "identity",
        }
    }

    fn execute(self, diagnostics: &mut Diagnostics) -> Result<Data> {
        let limits = ResourceLimits::default();
        match self {
            Self::Version => Ok(Data::Version {
                version: env!("CARGO_PKG_VERSION"),
            }),
            Self::Capabilities => Ok(Data::Capabilities(capabilities::capabilities())),
            Self::Info { input } => Ok(Data::Info(codec::load(&input, &limits, diagnostics)?.info)),
            Self::Run { image, pipeline } => {
                let pipeline = timed(&mut diagnostics.timings.validation_ms, || {
                    Pipeline::from_file(&pipeline, &limits)
                })?;
                execute_image(image, pipeline, &limits, diagnostics)
            }
            Self::Identity(image) => {
                let pipeline = timed(&mut diagnostics.timings.validation_ms, || {
                    let cwd = std::env::current_dir().map_err(|e| {
                        PicError::io("read working directory", std::path::Path::new("."), e)
                    })?;
                    Pipeline::single(OperationSpec::identity(), &cwd, &limits)
                })?;
                execute_image(image, pipeline, &limits, diagnostics)
            }
        }
    }
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
            output: &image.output,
            pipeline: &pipeline,
            encoding: EncodeOptions {
                format: image.format,
                jpeg_quality: image.jpeg_quality,
            },
            overwrite: image.overwrite,
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
