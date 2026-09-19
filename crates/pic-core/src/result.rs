use std::time::Instant;

use serde::Serialize;

use crate::PicError;

pub const RESULT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize)]
pub struct Warning {
    pub code: &'static str,
    pub message: &'static str,
}

/// Wall-clock milliseconds, including failed attempts; zero means no work in that stage.
#[derive(Debug, Default, Clone, Serialize)]
pub struct Timings {
    pub validation_ms: f64,
    pub read_ms: f64,
    pub decode_ms: f64,
    pub process_ms: f64,
    pub encode_ms: f64,
    pub write_ms: f64,
    pub total_ms: f64,
}

/// Always accounts for a stage, including when its closure returns an error.
pub fn timed<T>(stage: &mut f64, action: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = action();
    *stage += start.elapsed().as_secs_f64() * 1000.0;
    result
}

#[derive(Debug, Default)]
pub struct Diagnostics {
    pub timings: Timings,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Serialize)]
pub struct Envelope<T: Serialize> {
    pub schema_version: u32,
    pub engine_version: &'static str,
    pub command: String,
    pub ok: bool,
    pub data: Option<T>,
    pub error: Option<PicError>,
    pub warnings: Vec<Warning>,
    pub timings: Timings,
}

impl<T: Serialize> Envelope<T> {
    pub fn new(
        command: impl Into<String>,
        result: crate::Result<T>,
        diagnostics: Diagnostics,
    ) -> Self {
        let (data, error) = match result {
            Ok(data) => (Some(data), None),
            Err(error) => (None, Some(error)),
        };
        Self {
            schema_version: RESULT_SCHEMA_VERSION,
            engine_version: env!("CARGO_PKG_VERSION"),
            command: command.into(),
            ok: error.is_none(),
            data,
            error,
            warnings: diagnostics.warnings,
            timings: diagnostics.timings,
        }
    }
}
