use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use image::{ImageEncoder, Rgb, RgbImage, Rgba, RgbaImage};
use serde_json::Value;
use tempfile::TempDir;

fn binary() -> PathBuf {
    // Run the exact same behavioral suite against a separately built release executable.
    std::env::var_os("PIC_CLI_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_pic-cli")))
}

fn call(dir: &Path, args: &[&str]) -> Output {
    Command::new(binary())
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn json(dir: &Path, args: &[&str]) -> (Output, Value) {
    let output = Command::new(binary())
        .current_dir(dir)
        .arg("--json")
        .args(args)
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{args:?}: {error}; stdout={:?}, stderr={:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(String::from_utf8_lossy(&output.stdout).lines().count(), 1);
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["engine_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(value["ok"].as_bool().unwrap(), output.status.success());
    assert!(value["warnings"].is_array());
    for stage in [
        "validation_ms",
        "read_ms",
        "decode_ms",
        "process_ms",
        "encode_ms",
        "write_ms",
        "total_ms",
    ] {
        assert!(
            value["timings"][stage]
                .as_f64()
                .is_some_and(|ms| ms.is_finite() && ms >= 0.0),
            "{stage}: {value}"
        );
    }
    if output.status.success() {
        assert!(value["error"].is_null());
    } else {
        assert!(value["data"].is_null());
    }
    (output, value)
}

fn success(dir: &Path, args: &[&str]) -> Value {
    let (output, value) = json(dir, args);
    assert!(output.status.success(), "{args:?}: {value}");
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    value
}

fn failure(dir: &Path, args: &[&str], code: &str) -> Value {
    let (output, value) = json(dir, args);
    assert!(!output.status.success(), "{args:?}");
    assert_eq!(value["error"]["code"], code, "{value}");
    value
}

fn fixture() -> (TempDir, RgbaImage) {
    let dir = tempfile::tempdir().unwrap();
    let png = RgbaImage::from_fn(16, 8, |x, y| {
        Rgba([
            (x * 17) as u8,
            (y * 31) as u8,
            89,
            if y == 0 { 0 } else { (x * 17) as u8 },
        ])
    });
    png.save(dir.path().join("input.png")).unwrap();
    let jpeg = RgbImage::from_pixel(16, 8, Rgb([64, 128, 192]));
    let mut file = fs::File::create(dir.path().join("input.jpg")).unwrap();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, 100)
        .encode(jpeg.as_raw(), 16, 8, image::ExtendedColorType::Rgb8)
        .unwrap();
    fs::write(
        dir.path().join("empty.json"),
        r#"{"schema_version":1,"operations":[]}"#,
    )
    .unwrap();
    fs::write(dir.path().join("identity.json"), r#"{"schema_version":1,"operations":[{"op":"identity","op_version":1,"target":"canvas","params":{}}]}"#).unwrap();
    (dir, png)
}

fn assert_no_temporaries(dir: &Path) {
    for entry in fs::read_dir(dir).unwrap() {
        assert!(
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".pic-")
        );
    }
}

#[test]
fn help_version_and_capabilities_are_truthful_and_json_safe() {
    let dir = tempfile::tempdir().unwrap();
    for args in [&["--help"][..], &["help"], &["help", "run"]] {
        let output = call(dir.path(), args);
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("Usage:"));
        let value = success(dir.path(), args);
        assert_eq!(value["command"], "help");
        assert!(value["data"]["help"].as_str().unwrap().contains("Usage:"));
    }
    for args in [&["--version"][..], &["version"]] {
        let output = call(dir.path(), args);
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            concat!("pic-cli ", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(
            success(dir.path(), args)["data"]["version"],
            env!("CARGO_PKG_VERSION")
        );
    }
    let value = success(dir.path(), &["capabilities"]);
    assert_eq!(value["data"]["operations"].as_array().unwrap().len(), 1);
    assert_eq!(value["data"]["operations"][0]["op"], "identity");
    let items = value["data"]["capabilities"].as_array().unwrap();
    for status in ["supported", "partial", "not_implemented"] {
        assert!(
            items
                .iter()
                .any(|capability| capability["status"] == status)
        );
    }
    let smart = items
        .iter()
        .find(|item| item["id"] == "smart_editing")
        .unwrap();
    assert_eq!(smart["status"], "not_implemented");
    assert_eq!(smart["scope"], "roadmap");
    let output = call(dir.path(), &["capabilities", "--json"]);
    assert!(output.status.success());
    assert!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["ok"]
            .as_bool()
            .unwrap()
    );
}

#[test]
fn empty_png_pipeline_decodes_and_publishes_exact_rgba_pixels() {
    let (dir, expected) = fixture();
    let info = success(dir.path(), &["info", "input.png"]);
    assert_eq!(info["data"]["format"], "png");
    assert_eq!(info["data"]["width"], 16);
    assert_eq!(info["data"]["height"], 8);
    assert_eq!(info["data"]["has_transparency"], true);
    let value = success(
        dir.path(),
        &[
            "run",
            "--input",
            "input.png",
            "--pipeline",
            "empty.json",
            "--output",
            "output.png",
        ],
    );
    assert_eq!(value["data"]["operations_applied"], 0);
    assert_eq!(value["data"]["steps"], serde_json::json!([]));
    assert_eq!(
        value["data"]["output"],
        dir.path().join("output.png").to_str().unwrap()
    );
    assert_eq!(
        image::open(dir.path().join("output.png"))
            .unwrap()
            .into_rgba8(),
        expected
    );
    assert_no_temporaries(dir.path());
}

#[test]
fn jpeg_info_export_and_conversion_use_real_decoded_samples() {
    let (dir, _) = fixture();
    let info = success(dir.path(), &["info", "input.jpg"]);
    assert_eq!(info["data"]["format"], "jpeg");
    assert_eq!(info["data"]["bit_depth"], 8);
    assert_eq!(info["data"]["has_alpha"], false);
    assert_eq!(info["data"]["width"], 16);
    let original = image::open(dir.path().join("input.jpg"))
        .unwrap()
        .into_rgb8();
    let value = success(
        dir.path(),
        &[
            "run",
            "--input",
            "input.jpg",
            "--pipeline",
            "empty.json",
            "--output",
            "result.jpg",
            "--jpeg-quality",
            "100",
        ],
    );
    assert_eq!(value["data"]["format"], "jpeg");
    assert_eq!(value["data"]["jpeg_quality"], 100);
    let output = image::open(dir.path().join("result.jpg"))
        .unwrap()
        .into_rgb8();
    assert_eq!(output.dimensions(), original.dimensions());
    assert!(
        output
            .as_raw()
            .iter()
            .zip(original.as_raw())
            .all(|(a, b)| a.abs_diff(*b) <= 3)
    );
    success(
        dir.path(),
        &[
            "run",
            "--input",
            "input.jpg",
            "--pipeline",
            "empty.json",
            "--output",
            "converted.png",
        ],
    );
    assert_eq!(
        image::open(dir.path().join("converted.png"))
            .unwrap()
            .into_rgb8(),
        original
    );
    success(
        dir.path(),
        &[
            "identity",
            "--input",
            "converted.png",
            "--output",
            "back.jpg",
        ],
    );
    assert_eq!(
        image::open(dir.path().join("back.jpg")).unwrap().width(),
        16
    );
}

#[test]
fn single_operation_and_pipeline_share_results_and_use_pipeline_resource_base() {
    let (dir, expected) = fixture();
    fs::create_dir(dir.path().join("pipelines")).unwrap();
    fs::rename(
        dir.path().join("identity.json"),
        dir.path().join("pipelines/identity.json"),
    )
    .unwrap();
    let single = success(
        dir.path(),
        &["identity", "--input", "input.png", "--output", "single.png"],
    );
    let pipeline = success(
        dir.path(),
        &[
            "run",
            "--input",
            "input.png",
            "--pipeline",
            "pipelines/identity.json",
            "--output",
            "pipeline.png",
        ],
    );
    assert_eq!(single["data"]["steps"], pipeline["data"]["steps"]);
    assert_eq!(single["data"]["operations_applied"], 1);
    assert_eq!(
        pipeline["data"]["resource_base"],
        dir.path().join("pipelines").to_str().unwrap()
    );
    for path in ["single.png", "pipeline.png"] {
        assert_eq!(
            image::open(dir.path().join(path)).unwrap().into_rgba8(),
            expected
        );
    }
}

#[test]
fn overwrite_is_explicit_and_in_place_processing_is_atomic() {
    let (dir, expected) = fixture();
    let original_bytes = fs::read(dir.path().join("input.png")).unwrap();
    failure(
        dir.path(),
        &["identity", "--input", "input.png", "--output", "input.png"],
        "output_exists",
    );
    assert_eq!(
        fs::read(dir.path().join("input.png")).unwrap(),
        original_bytes
    );
    success(
        dir.path(),
        &[
            "identity",
            "--input",
            "input.png",
            "--output",
            "input.png",
            "--overwrite",
        ],
    );
    assert_eq!(
        image::open(dir.path().join("input.png"))
            .unwrap()
            .into_rgba8(),
        expected
    );
    fs::write(dir.path().join("existing.jpg"), b"keep me").unwrap();
    failure(
        dir.path(),
        &[
            "identity",
            "--input",
            "input.png",
            "--output",
            "existing.jpg",
            "--overwrite",
        ],
        "alpha_not_supported",
    );
    assert_eq!(
        fs::read(dir.path().join("existing.jpg")).unwrap(),
        b"keep me"
    );
    failure(
        dir.path(),
        &["identity", "--input", "input.png", "--output", "new.jpg"],
        "alpha_not_supported",
    );
    assert!(!dir.path().join("new.jpg").exists());
    assert_no_temporaries(dir.path());
}

#[test]
fn invalid_files_formats_and_paths_do_not_publish_an_output() {
    let (dir, _) = fixture();
    fs::write(dir.path().join("bad.png"), b"not an image").unwrap();
    fs::write(dir.path().join("truncated.png"), b"\x89PNG\r\n\x1a\n").unwrap();
    fs::write(dir.path().join("gif.png"), b"GIF89a\x01\x00\x01\x00").unwrap();
    for (input, code) in [
        ("missing.png", "file_not_found"),
        ("bad.png", "unsupported_format"),
        ("truncated.png", "decode_failed"),
        ("gif.png", "unsupported_format"),
        (".", "invalid_argument"),
    ] {
        failure(
            dir.path(),
            &["identity", "--input", input, "--output", "output.png"],
            code,
        );
        assert!(!dir.path().join("output.png").exists());
    }
    failure(
        dir.path(),
        &[
            "identity",
            "--input",
            "input.jpg",
            "--output",
            "missing/output.png",
        ],
        "file_not_found",
    );
    failure(
        dir.path(),
        &[
            "identity",
            "--input",
            "input.jpg",
            "--output",
            "output.webp",
        ],
        "unsupported_format",
    );
    fs::create_dir(dir.path().join("output.png")).unwrap();
    failure(
        dir.path(),
        &[
            "identity",
            "--input",
            "input.jpg",
            "--output",
            "output.png",
            "--overwrite",
        ],
        "invalid_argument",
    );
    assert_no_temporaries(dir.path());
}

#[test]
fn input_format_is_sniffed_and_output_format_can_be_explicit() {
    let (dir, expected) = fixture();
    fs::rename(
        dir.path().join("input.png"),
        dir.path().join("actually-png.jpg"),
    )
    .unwrap();
    assert_eq!(
        success(dir.path(), &["info", "actually-png.jpg"])["data"]["format"],
        "png"
    );
    success(
        dir.path(),
        &[
            "identity",
            "--input",
            "actually-png.jpg",
            "--output",
            "output.bin",
            "--format",
            "png",
        ],
    );
    let bytes = fs::read(dir.path().join("output.bin")).unwrap();
    assert_eq!(
        image::load_from_memory(&bytes).unwrap().into_rgba8(),
        expected
    );
}

#[test]
fn grayscale_alpha_and_opaque_grayscale_inputs_expand_correctly() {
    let dir = tempfile::tempdir().unwrap();
    let gray = image::ImageBuffer::from_fn(4, 2, |x, y| {
        image::LumaA([(x * 85) as u8, if y == 0 { 0 } else { 255 }])
    });
    gray.save(dir.path().join("gray.png")).unwrap();
    let info = success(dir.path(), &["info", "gray.png"]);
    assert_eq!(info["data"]["color_type"], "gray_alpha");
    success(
        dir.path(),
        &["identity", "--input", "gray.png", "--output", "rgba.png"],
    );
    let output = image::open(dir.path().join("rgba.png"))
        .unwrap()
        .into_rgba8();
    for (pixel, expected) in output.pixels().zip(gray.pixels()) {
        assert_eq!(
            pixel.0,
            [expected[0], expected[0], expected[0], expected[1]]
        );
    }
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 100)
        .encode(&[127; 32], 8, 4, image::ExtendedColorType::L8)
        .unwrap();
    fs::write(dir.path().join("gray.jpg"), bytes).unwrap();
    assert_eq!(
        success(dir.path(), &["info", "gray.jpg"])["data"]["color_type"],
        "gray"
    );
    success(
        dir.path(),
        &[
            "identity",
            "--input",
            "gray.jpg",
            "--output",
            "gray-rgb.png",
        ],
    );
    let output = image::open(dir.path().join("gray-rgb.png"))
        .unwrap()
        .into_rgb8();
    assert!(
        output
            .pixels()
            .all(|p| p[0] == p[1] && p[1] == p[2] && p[0].abs_diff(127) <= 1)
    );
}

#[test]
fn oversized_headers_and_pipeline_files_fail_before_pixel_allocation() {
    let (dir, _) = fixture();
    let original = fs::read(dir.path().join("input.png")).unwrap();
    let mut header = original[16..29].to_vec();
    header[..4].copy_from_slice(&32_769u32.to_be_bytes());
    let mut bytes = original[..8].to_vec();
    bytes.extend(png_chunk(b"IHDR", &header));
    bytes.extend_from_slice(&original[33..]);
    fs::write(dir.path().join("huge.png"), bytes).unwrap();
    failure(
        dir.path(),
        &["identity", "--input", "huge.png", "--output", "out.png"],
        "resource_limit",
    );
    fs::write(dir.path().join("huge.json"), vec![b' '; 1024 * 1024 + 1]).unwrap();
    failure(
        dir.path(),
        &[
            "run",
            "--input",
            "input.png",
            "--pipeline",
            "huge.json",
            "--output",
            "out.png",
        ],
        "resource_limit",
    );
    assert!(!dir.path().join("out.png").exists());
    assert_no_temporaries(dir.path());
}

#[test]
fn truncated_jpeg_and_metadata_after_scan_are_not_silently_accepted() {
    let (dir, _) = fixture();
    let original = fs::read(dir.path().join("input.jpg")).unwrap();
    let mut bytes = original[..original.len() - 2].to_vec();
    fs::write(dir.path().join("truncated.jpg"), &bytes).unwrap();
    failure(
        dir.path(),
        &[
            "identity",
            "--input",
            "truncated.jpg",
            "--output",
            "out.png",
        ],
        "decode_failed",
    );
    bytes.extend_from_slice(b"\xff\xe1\x00\x08Exif\0\0\xff\xd9");
    fs::write(dir.path().join("late-exif.jpg"), bytes).unwrap();
    failure(
        dir.path(),
        &[
            "identity",
            "--input",
            "late-exif.jpg",
            "--output",
            "out.png",
        ],
        "unsupported_metadata",
    );
    assert!(!dir.path().join("out.png").exists());
}

#[test]
fn invalid_json_and_operations_fail_before_image_io() {
    let (dir, _) = fixture();
    let cases = [
        ("{", "invalid_json"),
        (
            r#"{"schema_version":1,"operations":[],"extra":true}"#,
            "invalid_json",
        ),
        (
            r#"{"schema_version":999,"operations":[]}"#,
            "unsupported_version",
        ),
        (
            r#"{"schema_version":1,"operations":[{"op":"resize","op_version":1,"target":"canvas","params":{}}]}"#,
            "unknown_operation",
        ),
        (
            r#"{"schema_version":1,"operations":[{"op":"identity","op_version":99,"target":"canvas","params":{}}]}"#,
            "unsupported_version",
        ),
        (
            r#"{"schema_version":1,"operations":[{"op":"identity","op_version":1,"target":"canvas","params":{"foo":1}}]}"#,
            "invalid_argument",
        ),
    ];
    for (contents, code) in cases {
        fs::write(dir.path().join("bad.json"), contents).unwrap();
        let value = failure(
            dir.path(),
            &[
                "run",
                "--input",
                "missing.png",
                "--pipeline",
                "bad.json",
                "--output",
                "output.png",
            ],
            code,
        );
        assert_eq!(value["timings"]["read_ms"], 0.0);
        assert!(!dir.path().join("output.png").exists());
    }
    failure(
        dir.path(),
        &[
            "run",
            "--input",
            "input.png",
            "--pipeline",
            "missing.json",
            "--output",
            "output.png",
        ],
        "file_not_found",
    );
}

#[test]
fn argument_errors_are_versioned_json_and_return_nonzero() {
    let (dir, _) = fixture();
    for args in [
        &["bogus"][..],
        &["run"],
        &["info"],
        &["--wat"],
        &[
            "identity",
            "--input",
            "input.jpg",
            "--output",
            "out.jpg",
            "--jpeg-quality",
            "0",
        ],
        &[
            "identity",
            "--input",
            "input.jpg",
            "--output",
            "out.jpg",
            "--jpeg-quality",
            "101",
        ],
        &[
            "identity",
            "--input",
            "input.jpg",
            "--output",
            "out.jpg",
            "--jpeg-quality",
            "NaN",
        ],
    ] {
        failure(dir.path(), args, "invalid_argument");
    }
    failure(
        dir.path(),
        &[
            "identity",
            "--input",
            "input.png",
            "--output",
            "out.png",
            "--jpeg-quality",
            "90",
        ],
        "invalid_argument",
    );
    assert!(!dir.path().join("out.png").exists());
    assert!(!dir.path().join("out.jpg").exists());
}

// Minimal chunk construction keeps metadata fixtures generated in temporary directories.
fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    chunk.extend_from_slice(kind);
    chunk.extend_from_slice(data);
    let mut crc = u32::MAX;
    for byte in &chunk[4..] {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    chunk.extend_from_slice(&(!crc).to_be_bytes());
    chunk
}

#[test]
fn unsupported_color_orientation_animation_and_bit_depth_are_explicit_errors() {
    let (dir, _) = fixture();
    let original = fs::read(dir.path().join("input.png")).unwrap();
    for (kind, data, code) in [
        (b"gAMA", vec![0, 0, 177, 143], "unsupported_color"),
        (b"iCCP", vec![1], "unsupported_color"),
        (b"cHRM", vec![0; 32], "unsupported_color"),
        (b"eXIf", vec![0; 26], "unsupported_metadata"),
        (b"acTL", vec![0, 0, 0, 2, 0, 0, 0, 0], "unsupported_format"),
    ] {
        let mut bytes = original[..33].to_vec();
        bytes.extend(png_chunk(kind, &data));
        bytes.extend_from_slice(&original[33..]);
        fs::write(dir.path().join("tagged.png"), bytes).unwrap();
        failure(
            dir.path(),
            &["identity", "--input", "tagged.png", "--output", "out.png"],
            code,
        );
        assert!(!dir.path().join("out.png").exists());
    }
    let mut tagged = original[..33].to_vec();
    tagged.extend(png_chunk(b"sRGB", &[0]));
    tagged.extend_from_slice(&original[33..]);
    fs::write(dir.path().join("srgb.png"), tagged).unwrap();
    let info = success(dir.path(), &["info", "srgb.png"]);
    assert_eq!(info["data"]["color_source"], "declared_srgb");
    assert!(info["warnings"].as_array().unwrap().is_empty());
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(&[0; 8], 1, 1, image::ExtendedColorType::Rgba16)
        .unwrap();
    fs::write(dir.path().join("sixteen.png"), bytes).unwrap();
    failure(dir.path(), &["info", "sixteen.png"], "unsupported_color");
    let original = fs::read(dir.path().join("input.jpg")).unwrap();
    for (marker, data, code) in [
        (0xe1, b"Exif\0\0".as_slice(), "unsupported_metadata"),
        (0xe2, b"ICC_PROFILE\0".as_slice(), "unsupported_color"),
    ] {
        let mut bytes = original[..2].to_vec();
        bytes.extend([0xff, marker]);
        bytes.extend_from_slice(&((data.len() + 2) as u16).to_be_bytes());
        bytes.extend_from_slice(data);
        bytes.extend_from_slice(&original[2..]);
        fs::write(dir.path().join("tagged.jpg"), bytes).unwrap();
        failure(dir.path(), &["info", "tagged.jpg"], code);
    }
}

#[cfg(unix)]
#[test]
fn output_symlinks_and_non_utf8_paths_never_cause_unreported_writes() {
    use std::os::unix::{ffi::OsStringExt, fs::symlink};
    let (dir, _) = fixture();
    fs::write(dir.path().join("keep.png"), b"keep me").unwrap();
    symlink(dir.path().join("keep.png"), dir.path().join("link.png")).unwrap();
    failure(
        dir.path(),
        &[
            "identity",
            "--input",
            "input.png",
            "--output",
            "link.png",
            "--overwrite",
        ],
        "invalid_argument",
    );
    assert_eq!(fs::read(dir.path().join("keep.png")).unwrap(), b"keep me");
    let bad_path = dir
        .path()
        .join(std::ffi::OsString::from_vec(b"bad\xff.png".to_vec()));
    let output = Command::new(binary())
        .current_dir(dir.path())
        .args(["--json", "identity", "--input", "input.png", "--output"])
        .arg(&bad_path)
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!output.status.success());
    assert_eq!(value["error"]["code"], "invalid_argument");
    assert!(!bad_path.exists());
    assert_no_temporaries(dir.path());
}
