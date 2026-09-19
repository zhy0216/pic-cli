//! A small startup/codec baseline, not a benchmark of future editing algorithms.
use std::{collections::BTreeMap, fs, path::Path, process::Command, time::Instant};

use image::{Rgb, RgbImage, Rgba, RgbaImage};
use serde_json::{Value, json};

const RUNS: usize = 30;

fn distribution(mut samples: Vec<f64>) -> Value {
    samples.sort_by(f64::total_cmp);
    json!({ "p50": samples[(RUNS / 2) - 1], "p95": samples[(RUNS * 95).div_ceil(100) - 1] })
}

fn measure(binary: &Path, dir: &Path, args: &[&str]) -> Value {
    let mut wall = Vec::new();
    let mut stages: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for iteration in 0..=RUNS {
        let start = Instant::now();
        let output = Command::new(binary)
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        assert!(
            output.status.success(),
            "{args:?}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        if args == ["--version"] {
            assert_eq!(
                String::from_utf8_lossy(&output.stdout).trim(),
                concat!("pic-cli ", env!("CARGO_PKG_VERSION"))
            );
        } else {
            let result: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(result["schema_version"], 1);
            assert_eq!(result["ok"], true);
            assert_eq!(result["data"]["width"], 64);
            assert_eq!(result["data"]["height"], 48);
            if args.contains(&"run") {
                assert_eq!(result["data"]["operations_applied"], 0);
            }
            if iteration > 0 {
                for (name, value) in result["timings"].as_object().unwrap() {
                    stages
                        .entry(name.clone())
                        .or_default()
                        .push(value.as_f64().unwrap());
                }
            }
        }
        if iteration > 0 {
            wall.push(elapsed);
        }
    }
    let stages: BTreeMap<_, _> = stages
        .into_iter()
        .map(|(name, samples)| (name, distribution(samples)))
        .collect();
    json!({ "args": args, "runs": RUNS, "wall_ms": distribution(wall), "stages_ms": stages })
}

fn main() {
    let binary = std::env::args_os()
        .nth(1)
        .expect("usage: foundation_baseline /absolute/path/to/release/pic-cli");
    let binary = fs::canonicalize(binary).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let png = RgbaImage::from_fn(64, 48, |x, y| {
        Rgba([(x * 4) as u8, (y * 5) as u8, 89, (x * 4) as u8])
    });
    png.save(dir.path().join("input.png")).unwrap();
    let jpeg = RgbImage::from_pixel(64, 48, Rgb([64, 128, 192]));
    let mut file = fs::File::create(dir.path().join("input.jpg")).unwrap();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, 100)
        .encode(jpeg.as_raw(), 64, 48, image::ExtendedColorType::Rgb8)
        .unwrap();
    drop(file);
    fs::write(
        dir.path().join("pipeline.json"),
        r#"{"schema_version":1,"operations":[]}"#,
    )
    .unwrap();
    let results = [
        measure(&binary, dir.path(), &["--version"]),
        measure(&binary, dir.path(), &["info", "input.png", "--json"]),
        measure(&binary, dir.path(), &["info", "input.jpg", "--json"]),
        measure(
            &binary,
            dir.path(),
            &[
                "run",
                "--input",
                "input.png",
                "--pipeline",
                "pipeline.json",
                "--output",
                "output.png",
                "--overwrite",
                "--json",
            ],
        ),
        measure(
            &binary,
            dir.path(),
            &[
                "run",
                "--input",
                "input.jpg",
                "--pipeline",
                "pipeline.json",
                "--output",
                "output.jpg",
                "--jpeg-quality",
                "90",
                "--overwrite",
                "--json",
            ],
        ),
    ];
    assert_eq!(
        image::open(dir.path().join("output.png"))
            .unwrap()
            .into_rgba8(),
        png
    );
    let original = image::open(dir.path().join("input.jpg"))
        .unwrap()
        .into_rgb8();
    let output = image::open(dir.path().join("output.jpg"))
        .unwrap()
        .into_rgb8();
    assert!(
        output
            .as_raw()
            .iter()
            .zip(original.as_raw())
            .all(|(a, b)| a.abs_diff(*b) <= 3)
    );
    println!("{}", serde_json::to_string_pretty(&json!({ "fixture": "64x48 generated PNG gradient with alpha; uniform RGB JPEG", "cache": "one warmup then 30 fresh CLI processes; warm filesystem cache; no fsync", "results": results })).unwrap());
}
