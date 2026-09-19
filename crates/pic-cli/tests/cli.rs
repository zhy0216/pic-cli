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
    assert_eq!(value["data"]["operations"].as_array().unwrap().len(), 30);
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
            r#"{"schema_version":1,"operations":[{"op":"future-op","op_version":1,"target":"canvas","params":{}}]}"#,
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

fn write_pipeline(dir: &Path, operations: Vec<Value>) {
    fs::write(
        dir.join("geometry.json"),
        serde_json::to_vec(&serde_json::json!({"schema_version":1,"operations":operations}))
            .unwrap(),
    )
    .unwrap();
}

fn geometry_op(op: &str, params: Value) -> Value {
    serde_json::json!({"op":op,"op_version":1,"target":"canvas","params":params})
}

#[test]
fn every_photo_command_matches_its_explicit_json_parameters_and_pixels() {
    use serde_json::json;
    let (dir, _) = fixture();
    let capabilities = success(dir.path(), &["capabilities"]);
    let operations = capabilities["data"]["operations"].as_array().unwrap();
    let levels = json!({"channel":"rgb","input_black":0,"input_white":1,"gamma":1,"output_black":0,"output_white":1});
    for (op, arguments, params) in [
        (
            "adjust",
            vec![],
            json!({"exposure":0,"brightness":0,"contrast":1,"saturation":1}),
        ),
        (
            "adjust",
            vec!["--exposure", "1"],
            json!({"exposure":1,"brightness":0,"contrast":1,"saturation":1}),
        ),
        (
            "adjust",
            vec!["--brightness", "-0.125"],
            json!({"exposure":0,"brightness":-0.125,"contrast":1,"saturation":1}),
        ),
        (
            "adjust",
            vec!["--contrast", "1.5"],
            json!({"exposure":0,"brightness":0,"contrast":1.5,"saturation":1}),
        ),
        (
            "adjust",
            vec!["--saturation", "0"],
            json!({"exposure":0,"brightness":0,"contrast":1,"saturation":0}),
        ),
        (
            "adjust",
            vec![
                "--exposure",
                "-0.25",
                "--brightness",
                "0.125",
                "--contrast",
                "0.75",
                "--saturation",
                "1.2",
            ],
            json!({"exposure":-0.25,"brightness":0.125,"contrast":0.75,"saturation":1.2}),
        ),
        ("levels", vec![], levels),
        (
            "levels",
            vec![
                "--channel",
                "red",
                "--input-black",
                "0.1",
                "--input-white",
                "0.9",
                "--gamma",
                "1.5",
                "--output-black",
                "0.125",
                "--output-white",
                "0.875",
            ],
            json!({"channel":"red","input_black":0.1,"input_white":0.9,"gamma":1.5,"output_black":0.125,"output_white":0.875}),
        ),
        (
            "curves",
            vec![],
            json!({"channel":"rgb","points":[[0,0],[1,1]]}),
        ),
        (
            "curves",
            vec![
                "--channel",
                "green",
                "--points",
                "[[0,0],[0.25,0.5],[0.5,0.25],[1,1]]",
            ],
            json!({"channel":"green","points":[[0,0],[0.25,0.5],[0.5,0.25],[1,1]]}),
        ),
        (
            "curves",
            vec!["--channel", "blue", "--points", "[[0,0.25],[1,0.75]]"],
            json!({"channel":"blue","points":[[0,0.25],[1,0.75]]}),
        ),
        ("grayscale", vec![], json!({})),
        ("invert", vec![], json!({})),
        ("blur", vec![], json!({"sigma":1})),
        ("blur", vec!["--sigma", "0"], json!({"sigma":0})),
        ("blur", vec!["--sigma", "0.5"], json!({"sigma":0.5})),
        ("sharpen", vec![], json!({"sigma":1,"amount":1})),
        (
            "sharpen",
            vec!["--sigma", "0.5", "--amount", "2"],
            json!({"sigma":0.5,"amount":2}),
        ),
    ] {
        let capability = operations.iter().find(|value| value["op"] == op).unwrap();
        assert_eq!(capability["op_version"], 1);
        assert_eq!(capability["params"]["additionalProperties"], false);
        let help = success(dir.path(), &["help", op]);
        for parameter in params.as_object().unwrap().keys() {
            assert!(
                help["data"]["help"]
                    .as_str()
                    .unwrap()
                    .contains(&format!("--{}", parameter.replace('_', "-")))
            );
            assert!(
                capability["params"]["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(parameter))
            );
        }
        let mut args = vec![
            op,
            "--input",
            "input.png",
            "--output",
            "single.png",
            "--overwrite",
        ];
        args.extend(arguments);
        let single = success(dir.path(), &args);
        write_pipeline(dir.path(), vec![geometry_op(op, params)]);
        let pipeline = success(
            dir.path(),
            &[
                "run",
                "--input",
                "input.png",
                "--pipeline",
                "geometry.json",
                "--output",
                "pipeline.png",
                "--overwrite",
            ],
        );
        assert_eq!(single["data"]["steps"], pipeline["data"]["steps"], "{op}");
        assert_eq!(
            image::open(dir.path().join("single.png"))
                .unwrap()
                .into_rgba8(),
            image::open(dir.path().join("pipeline.png"))
                .unwrap()
                .into_rgba8(),
            "{op}"
        );
    }
}

#[test]
fn geometry_adjustment_and_filters_export_known_pixels_to_png_and_jpeg() {
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    RgbaImage::from_raw(3, 1, vec![255, 0, 0, 255, 0, 0, 0, 128, 255, 255, 255, 128])
        .unwrap()
        .save(dir.path().join("input.png"))
        .unwrap();
    let operations = vec![
        geometry_op("crop", json!({"x":1,"y":0,"width":2,"height":1})),
        geometry_op("flip", json!({"axis":"horizontal"})),
        geometry_op(
            "rotate",
            json!({"degrees":90,"expand":true,"filter":"nearest","background":[0,0,0,0]}),
        ),
        geometry_op(
            "adjust",
            json!({"exposure":-1,"brightness":0,"contrast":1,"saturation":1}),
        ),
        geometry_op("blur", json!({"sigma":1})),
        geometry_op("sharpen", json!({"sigma":1,"amount":1})),
    ];
    write_pipeline(dir.path(), operations.clone());
    let result = success(
        dir.path(),
        &[
            "run",
            "--input",
            "input.png",
            "--pipeline",
            "geometry.json",
            "--output",
            "output.png",
        ],
    );
    assert_eq!(result["data"]["operations_applied"], 6);
    for (index, step) in result["data"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        assert_eq!(step["index"], index);
        assert_eq!(step["op"], operations[index]["op"]);
    }
    // sigma=1 tail T=0.30047486017377. After half exposure, blur and unsharp,
    // top linear RGB=0.5-T^2, bottom=T^2; sRGB bytes 171/85. Alpha stays 128.
    let expected = RgbaImage::from_raw(1, 2, vec![171, 171, 171, 128, 85, 85, 85, 128]).unwrap();
    assert_eq!(
        image::open(dir.path().join("output.png"))
            .unwrap()
            .into_rgba8(),
        expected
    );
    failure(
        dir.path(),
        &[
            "run",
            "--input",
            "input.png",
            "--pipeline",
            "geometry.json",
            "--output",
            "output.jpg",
        ],
        "alpha_not_supported",
    );
    assert!(!dir.path().join("output.jpg").exists());
    success(
        dir.path(),
        &[
            "run",
            "--input",
            "input.png",
            "--pipeline",
            "geometry.json",
            "--output",
            "output.jpg",
            "--jpeg-quality",
            "100",
            "--jpeg-background",
            "#ffffff",
        ],
    );
    let jpeg = image::open(dir.path().join("output.jpg"))
        .unwrap()
        .into_rgb8();
    assert_eq!(jpeg.dimensions(), (1, 2));
    for (pixel, expected) in jpeg.pixels().zip([218u8, 195]) {
        assert!(pixel.0.iter().all(|actual| actual.abs_diff(expected) <= 2));
    }
    assert_no_temporaries(dir.path());
}

#[test]
fn photo_pipelines_keep_step_order_and_never_quantize_or_clip_between_steps() {
    use serde_json::json;
    let (dir, original) = fixture();
    write_pipeline(
        dir.path(),
        vec![
            geometry_op(
                "adjust",
                json!({"exposure":20,"brightness":0,"contrast":1,"saturation":1}),
            ),
            geometry_op(
                "adjust",
                json!({"exposure":-20,"brightness":0,"contrast":1,"saturation":1}),
            ),
        ],
    );
    success(
        dir.path(),
        &[
            "run",
            "--input",
            "input.png",
            "--pipeline",
            "geometry.json",
            "--output",
            "roundtrip.png",
        ],
    );
    assert_eq!(
        image::open(dir.path().join("roundtrip.png"))
            .unwrap()
            .into_rgba8(),
        original
    );
    let mut operations = vec![
        geometry_op(
            "adjust",
            json!({"exposure":1,"brightness":0,"contrast":1,"saturation":1}),
        ),
        geometry_op(
            "adjust",
            json!({"exposure":0,"brightness":0.125,"contrast":1,"saturation":1}),
        ),
    ];
    let mut outputs = Vec::new();
    for name in ["first.png", "second.png"] {
        write_pipeline(dir.path(), operations.clone());
        success(
            dir.path(),
            &[
                "run",
                "--input",
                "input.png",
                "--pipeline",
                "geometry.json",
                "--output",
                name,
            ],
        );
        outputs.push(image::open(dir.path().join(name)).unwrap().into_rgba8());
        operations.reverse();
    }
    assert_ne!(outputs[0], outputs[1]);
    // First input channel is 0. Exposure then brightness gives linear 0.125 (99 sRGB);
    // brightness then exposure gives linear 0.25 (137 sRGB), even at alpha zero.
    assert_eq!(outputs[0].get_pixel(0, 0)[0], 99);
    assert_eq!(outputs[1].get_pixel(0, 0)[0], 137);
}

#[test]
fn photo_parameter_errors_and_runtime_overflow_never_publish_success() {
    use serde_json::json;
    let (dir, _) = fixture();
    for arguments in [
        vec!["adjust", "--exposure", "NaN"],
        vec!["adjust", "--brightness", "inf"],
        vec!["adjust", "--contrast", "-inf"],
        vec!["adjust", "--saturation", "-1"],
        vec!["adjust", "--exposure", "21"],
        vec!["adjust", "--brightness", "1.1"],
        vec!["adjust", "--contrast", "11"],
        vec!["adjust", "--saturation", "NaN"],
        vec!["levels", "--input-black", "1"],
        vec!["levels", "--gamma", "0"],
        vec!["levels", "--gamma", "NaN"],
        vec!["levels", "--output-black", "0.75", "--output-white", "0.25"],
        vec!["curves", "--channel", "alpha"],
        vec!["curves", "--points", "[[0,0],[0.5,NaN],[1,1]]"],
        vec!["curves", "--points", "[[0,0],[0.5,1],[0.5,0],[1,1]]"],
        vec!["grayscale", "--weights", "equal"],
        vec!["invert", "--channel", "alpha"],
        vec!["blur", "--sigma", "101"],
        vec!["blur", "--sigma", "NaN"],
        vec!["blur", "--edge", "wrap"],
        vec!["sharpen", "--amount", "inf"],
        vec!["sharpen", "--sigma", "-1"],
    ] {
        let mut args = arguments;
        args.extend(["--input", "missing.png", "--output", "absent.png"]);
        let error = failure(dir.path(), &args, "invalid_argument");
        assert_eq!(error["timings"]["read_ms"], 0.0);
        assert!(!dir.path().join("absent.png").exists());
        fs::write(dir.path().join("absent.png"), b"preserve").unwrap();
        args.push("--overwrite");
        failure(dir.path(), &args, "invalid_argument");
        assert_eq!(
            fs::read(dir.path().join("absent.png")).unwrap(),
            b"preserve"
        );
        fs::remove_file(dir.path().join("absent.png")).unwrap();
    }
    for (op, params) in [
        ("adjust", json!({"exposure":0})),
        ("levels", json!({"gamma":1})),
        (
            "curves",
            json!({"channel":"rgb","points":[[0,0],[1,1]],"interpolation":"cubic"}),
        ),
        ("blur", json!({"sigma":null})),
        ("blur", json!([1])),
        ("sharpen", json!({"sigma":1,"amount":"NaN"})),
        ("grayscale", json!({"ignored":true})),
        ("invert", json!([])),
    ] {
        write_pipeline(dir.path(), vec![geometry_op(op, params)]);
        let error = failure(
            dir.path(),
            &[
                "run",
                "--input",
                "missing.png",
                "--pipeline",
                "geometry.json",
                "--output",
                "absent.png",
            ],
            "invalid_argument",
        );
        assert!(
            error["error"]["message"]
                .as_str()
                .unwrap()
                .contains("operations[0]")
        );
        assert!(!dir.path().join("absent.png").exists());
    }
    for number in ["NaN", "Infinity", "-Infinity", "1e999"] {
        fs::write(dir.path().join("geometry.json"),format!(r#"{{"schema_version":1,"operations":[{{"op":"blur","op_version":1,"target":"canvas","params":{{"sigma":{number}}}}}]}}"#)).unwrap();
        failure(
            dir.path(),
            &[
                "run",
                "--input",
                "missing.png",
                "--pipeline",
                "geometry.json",
                "--output",
                "absent.png",
            ],
            "invalid_json",
        );
        assert!(!dir.path().join("absent.png").exists());
    }
    write_pipeline(
        dir.path(),
        vec![
            geometry_op(
                "adjust",
                json!({"exposure":20,"brightness":0,"contrast":1,"saturation":1})
            );
            7
        ],
    );
    for overwrite in [false, true] {
        let mut args = vec![
            "run",
            "--input",
            "input.png",
            "--pipeline",
            "geometry.json",
            "--output",
            "absent.png",
        ];
        if overwrite {
            fs::write(dir.path().join("absent.png"), b"preserve").unwrap();
            args.push("--overwrite");
        }
        let error = failure(dir.path(), &args, "invalid_argument");
        assert!(
            error["error"]["message"]
                .as_str()
                .unwrap()
                .contains("operations[6]")
        );
        assert_eq!(error["timings"]["encode_ms"], 0.0);
        assert_eq!(error["timings"]["write_ms"], 0.0);
        if overwrite {
            assert_eq!(
                fs::read(dir.path().join("absent.png")).unwrap(),
                b"preserve"
            );
        } else {
            assert!(!dir.path().join("absent.png").exists());
        }
    }
    assert_no_temporaries(dir.path());
}

#[test]
fn every_geometry_command_matches_a_json_step_in_separate_processes() {
    use serde_json::json;
    let (dir, _) = fixture();
    for (op, arguments, params) in [
        (
            "crop",
            vec!["--x", "2", "--y", "1", "--width", "4", "--height", "3"],
            json!({"x":2,"y":1,"width":4,"height":3}),
        ),
        (
            "resize",
            vec!["--width", "7"],
            json!({"width":7,"height":null,"filter":"bilinear"}),
        ),
        (
            "resize",
            vec!["--height", "3", "--filter", "nearest"],
            json!({"width":null,"height":3,"filter":"nearest"}),
        ),
        (
            "rotate",
            vec!["--degrees", "90"],
            json!({"degrees":90.0,"expand":true,"filter":"bilinear","background":[0,0,0,0]}),
        ),
        (
            "rotate",
            vec!["--degrees", "-37", "--background", "#0000ff80"],
            json!({"degrees":-37.0,"expand":true,"filter":"bilinear","background":[0,0,255,128]}),
        ),
        (
            "rotate",
            vec!["--degrees", "45", "--keep-size", "--filter", "nearest"],
            json!({"degrees":45.0,"expand":false,"filter":"nearest","background":[0,0,0,0]}),
        ),
        (
            "flip",
            vec!["--axis", "horizontal"],
            json!({"axis":"horizontal"}),
        ),
        (
            "flip",
            vec!["--axis", "vertical"],
            json!({"axis":"vertical"}),
        ),
        (
            "canvas",
            vec![
                "--width",
                "19",
                "--height",
                "10",
                "--background",
                "#1280ff",
                "--anchor",
                "bottom_right",
            ],
            json!({"width":19,"height":10,"anchor":"bottom_right","background":[18,128,255,255]}),
        ),
        (
            "canvas",
            vec!["--width", "7", "--height", "3"],
            json!({"width":7,"height":3,"anchor":"center","background":[0,0,0,0]}),
        ),
    ] {
        let mut args = vec![
            op,
            "--input",
            "input.png",
            "--output",
            "single.png",
            "--overwrite",
        ];
        args.extend(arguments);
        let single = success(dir.path(), &args);
        write_pipeline(dir.path(), vec![geometry_op(op, params)]);
        let pipeline = success(
            dir.path(),
            &[
                "run",
                "--input",
                "input.png",
                "--pipeline",
                "geometry.json",
                "--output",
                "pipeline.png",
                "--overwrite",
            ],
        );
        assert_eq!(single["data"]["steps"], pipeline["data"]["steps"], "{op}");
        let a = image::open(dir.path().join("single.png"))
            .unwrap()
            .into_rgba8();
        let b = image::open(dir.path().join("pipeline.png"))
            .unwrap()
            .into_rgba8();
        assert_eq!(a, b, "{op}");
    }
}

#[test]
fn multistep_pipeline_matches_sequential_cli_processes_and_known_coordinates() {
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    let original = RgbaImage::from_fn(3, 2, |x, y| {
        Rgba([(x * 80) as u8, (y * 150) as u8, 57, ((x + y) * 70) as u8])
    });
    original.save(dir.path().join("input.png")).unwrap();
    let steps = [
        (
            "crop",
            vec!["--x", "1", "--y", "0", "--width", "2", "--height", "2"],
            json!({"x":1,"y":0,"width":2,"height":2}),
        ),
        (
            "resize",
            vec!["--width", "4", "--height", "4", "--filter", "nearest"],
            json!({"width":4,"height":4,"filter":"nearest"}),
        ),
        (
            "rotate",
            vec!["--degrees", "90"],
            json!({"degrees":90.0,"expand":true,"filter":"bilinear","background":[0,0,0,0]}),
        ),
        (
            "flip",
            vec!["--axis", "horizontal"],
            json!({"axis":"horizontal"}),
        ),
        (
            "flip",
            vec!["--axis", "vertical"],
            json!({"axis":"vertical"}),
        ),
        (
            "canvas",
            vec![
                "--width",
                "6",
                "--height",
                "5",
                "--anchor",
                "bottom_right",
                "--background",
                "#01020304",
            ],
            json!({"width":6,"height":5,"anchor":"bottom_right","background":[1,2,3,4]}),
        ),
    ];
    let mut ops = vec![];
    for (index, (op, arguments, params)) in steps.into_iter().enumerate() {
        let input = if index == 0 { "input.png" } else { "step.png" };
        let mut args = vec![op, "--input", input, "--output", "step.png", "--overwrite"];
        args.extend(arguments);
        success(dir.path(), &args);
        ops.push(geometry_op(op, params));
    }
    write_pipeline(dir.path(), ops);
    let result = success(
        dir.path(),
        &[
            "run",
            "--input",
            "input.png",
            "--pipeline",
            "geometry.json",
            "--output",
            "pipeline.png",
        ],
    );
    assert_eq!(result["data"]["operations_applied"], 6);
    let output = image::open(dir.path().join("pipeline.png"))
        .unwrap()
        .into_rgba8();
    assert_eq!(
        output,
        image::open(dir.path().join("step.png"))
            .unwrap()
            .into_rgba8()
    );
    assert_eq!(output.dimensions(), (6, 5));
    // After crop/2x nearest/90 clockwise/both flips: [2,5] above [1,4], each doubled.
    for y in 0..5 {
        for x in 0..6 {
            let expected = if x < 2 || y < 1 {
                Rgba([1, 2, 3, 4])
            } else {
                let sx = if y < 3 { 2 } else { 1 };
                let sy = if x < 4 { 0 } else { 1 };
                *original.get_pixel(sx, sy)
            };
            assert_eq!(*output.get_pixel(x, y), expected, "({x},{y})");
        }
    }
}

fn exif_orientation(value: u16, little: bool) -> Vec<u8> {
    if little {
        let mut bytes = b"II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0".to_vec();
        bytes.extend(value.to_le_bytes());
        bytes.extend([0; 6]);
        bytes
    } else {
        let mut bytes = b"MM\0\x2a\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01".to_vec();
        bytes.extend(value.to_be_bytes());
        bytes.extend([0; 6]);
        bytes
    }
}

fn jpeg_segment(bytes: &[u8], marker: u8, payload: &[u8], late: bool) -> Vec<u8> {
    let offset = if late { bytes.len() - 2 } else { 2 };
    let mut output = bytes[..offset].to_vec();
    output.extend([0xff, marker]);
    output.extend(((payload.len() + 2) as u16).to_be_bytes());
    output.extend(payload);
    output.extend(&bytes[offset..]);
    output
}

#[test]
fn real_jpeg_exif_all_eight_orientations_are_normalized_before_info_and_crop() {
    let dir = tempfile::tempdir().unwrap();
    let source = RgbImage::from_fn(3, 2, |x, y| Rgb([(x * 100) as u8, (y * 180) as u8, 31]));
    let mut encoded = vec![];
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 100)
        .encode(source.as_raw(), 3, 2, image::ExtendedColorType::Rgb8)
        .unwrap();
    let stored = image::load_from_memory(&encoded).unwrap().into_rgb8();
    for (orientation, labels) in [
        (1, vec![0, 1, 2, 3, 4, 5]),
        (2, vec![2, 1, 0, 5, 4, 3]),
        (3, vec![5, 4, 3, 2, 1, 0]),
        (4, vec![3, 4, 5, 0, 1, 2]),
        (5, vec![0, 3, 1, 4, 2, 5]),
        (6, vec![3, 0, 4, 1, 5, 2]),
        (7, vec![5, 2, 4, 1, 3, 0]),
        (8, vec![2, 5, 1, 4, 0, 3]),
    ] {
        for little in [true, false] {
            let mut payload = b"Exif\0\0".to_vec();
            payload.extend(exif_orientation(orientation, little));
            // Metadata after entropy scans is also inspected and applied.
            fs::write(
                dir.path().join("tagged.jpg"),
                jpeg_segment(&encoded, 0xe1, &payload, !little),
            )
            .unwrap();
            let info = success(dir.path(), &["info", "tagged.jpg"]);
            let (width, height) = if orientation >= 5 { (2, 3) } else { (3, 2) };
            assert_eq!(info["data"]["width"], width);
            assert_eq!(info["data"]["height"], height);
            assert_eq!(info["data"]["stored_width"], 3);
            assert_eq!(info["data"]["stored_height"], 2);
            assert_eq!(info["data"]["exif_orientation"], orientation);
            success(
                dir.path(),
                &[
                    "identity",
                    "--input",
                    "tagged.jpg",
                    "--output",
                    "normalized.png",
                    "--overwrite",
                ],
            );
            let output = image::open(dir.path().join("normalized.png"))
                .unwrap()
                .into_rgb8();
            for (pixel, &index) in output.pixels().zip(&labels) {
                assert_eq!(
                    pixel,
                    stored.get_pixel(index % 3, index / 3),
                    "orientation {orientation}, little={little}"
                );
            }
            success(
                dir.path(),
                &[
                    "crop",
                    "--input",
                    "tagged.jpg",
                    "--output",
                    "crop.png",
                    "--overwrite",
                    "--x",
                    "0",
                    "--y",
                    "0",
                    "--width",
                    "1",
                    "--height",
                    "1",
                ],
            );
            assert_eq!(
                image::open(dir.path().join("crop.png"))
                    .unwrap()
                    .into_rgb8()
                    .get_pixel(0, 0),
                stored.get_pixel(labels[0] % 3, labels[0] / 3)
            );
            // Export has no EXIF left to apply a second time.
            assert!(
                success(dir.path(), &["info", "normalized.png"])["data"]["exif_orientation"]
                    .is_null()
            );
        }
    }
}

#[test]
fn png_exif_and_malformed_or_ambiguous_orientation_have_explicit_behavior() {
    let (dir, source) = fixture();
    let encoded = fs::read(dir.path().join("input.png")).unwrap();
    let mut png = encoded[..33].to_vec();
    png.extend(png_chunk(b"eXIf", &exif_orientation(6, true)));
    png.extend(&encoded[33..]);
    fs::write(dir.path().join("exif.png"), png).unwrap();
    success(
        dir.path(),
        &[
            "identity",
            "--input",
            "exif.png",
            "--output",
            "normalized.png",
        ],
    );
    let output = image::open(dir.path().join("normalized.png"))
        .unwrap()
        .into_rgba8();
    assert_eq!(output.dimensions(), (8, 16));
    assert_eq!(*output.get_pixel(0, 0), *source.get_pixel(0, 7));
    let jpeg = fs::read(dir.path().join("input.jpg")).unwrap();
    let mut duplicate = exif_orientation(1, true);
    duplicate[8] = 2;
    duplicate.splice(22..22, exif_orientation(6, true)[10..22].iter().copied());
    let mut bad_pointer = exif_orientation(1, true);
    bad_pointer[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut wrong_type = exif_orientation(1, true);
    wrong_type[12] = 4;
    for exif in [
        exif_orientation(0, true),
        exif_orientation(9, false),
        duplicate,
        bad_pointer,
        wrong_type,
    ] {
        let mut payload = b"Exif\0\0".to_vec();
        payload.extend(exif);
        fs::write(
            dir.path().join("bad.jpg"),
            jpeg_segment(&jpeg, 0xe1, &payload, false),
        )
        .unwrap();
        failure(dir.path(), &["info", "bad.jpg"], "unsupported_metadata");
    }
    let mut payload = b"Exif\0\0".to_vec();
    payload.extend(exif_orientation(1, true));
    let tagged = jpeg_segment(&jpeg, 0xe1, &payload, false);
    fs::write(
        dir.path().join("duplicate.jpg"),
        jpeg_segment(&tagged, 0xe1, &payload, true),
    )
    .unwrap();
    failure(
        dir.path(),
        &["info", "duplicate.jpg"],
        "unsupported_metadata",
    );
}

#[test]
fn exif_non_srgb_colors_and_unsupported_png_jpeg_representations_are_rejected() {
    let (dir, _) = fixture();
    let jpeg = fs::read(dir.path().join("input.jpg")).unwrap();
    // IFD0 points to an Exif sub-IFD containing ColorSpace (A001).
    for color in [1u16, 2, 65535] {
        let mut tiff = exif_orientation(1, true);
        tiff[10..12].copy_from_slice(&0x8769u16.to_le_bytes());
        tiff[12..14].copy_from_slice(&4u16.to_le_bytes());
        tiff[18..22].copy_from_slice(&26u32.to_le_bytes());
        let mut sub = exif_orientation(color, true)[8..].to_vec();
        sub[2..4].copy_from_slice(&0xa001u16.to_le_bytes());
        tiff.extend(sub);
        let mut payload = b"Exif\0\0".to_vec();
        payload.extend(tiff);
        fs::write(
            dir.path().join("color.jpg"),
            jpeg_segment(&jpeg, 0xe1, &payload, false),
        )
        .unwrap();
        if color == 1 {
            assert_eq!(
                success(dir.path(), &["info", "color.jpg"])["data"]["color_source"],
                "declared_srgb"
            );
        } else {
            failure(dir.path(), &["info", "color.jpg"], "unsupported_color");
        }
    }
    let frame = jpeg.windows(2).position(|b| b == [0xff, 0xc0]).unwrap();
    for (field, value) in [(frame + 4, 12), (frame + 9, 4)] {
        let mut bytes = jpeg.clone();
        bytes[field] = value;
        fs::write(dir.path().join("unsupported.jpg"), bytes).unwrap();
        failure(
            dir.path(),
            &["info", "unsupported.jpg"],
            "unsupported_color",
        );
    }
    let png = fs::read(dir.path().join("input.png")).unwrap();
    for (depth, color) in [(8, 3), (1, 0), (4, 0), (16, 6)] {
        let mut header = png[16..29].to_vec();
        header[8] = depth;
        header[9] = color;
        let mut bytes = png[..8].to_vec();
        bytes.extend(png_chunk(b"IHDR", &header));
        bytes.extend(&png[33..]);
        fs::write(dir.path().join("unsupported.png"), bytes).unwrap();
        failure(
            dir.path(),
            &["info", "unsupported.png"],
            "unsupported_color",
        );
    }
}

#[test]
fn png_compression_and_transparent_jpeg_policy_work_on_real_files() {
    let (dir, expected) = fixture();
    for level in ["0", "1", "6", "9"] {
        let result = success(
            dir.path(),
            &[
                "identity",
                "--input",
                "input.png",
                "--output",
                "compressed.png",
                "--overwrite",
                "--png-compression",
                level,
            ],
        );
        assert_eq!(
            result["data"]["png_compression"],
            level.parse::<u8>().unwrap()
        );
        assert_eq!(
            image::open(dir.path().join("compressed.png"))
                .unwrap()
                .into_rgba8(),
            expected
        );
    }
    let size = |level| {
        success(
            dir.path(),
            &[
                "identity",
                "--input",
                "input.png",
                "--output",
                "compressed.png",
                "--overwrite",
                "--png-compression",
                level,
            ],
        );
        fs::metadata(dir.path().join("compressed.png"))
            .unwrap()
            .len()
    };
    assert!(size("0") > size("9"));
    RgbaImage::from_pixel(8, 8, Rgba([255, 0, 0, 128]))
        .save(dir.path().join("alpha.png"))
        .unwrap();
    let result = success(
        dir.path(),
        &[
            "identity",
            "--input",
            "alpha.png",
            "--output",
            "flat.jpg",
            "--jpeg-background",
            "#0000ff",
            "--jpeg-quality",
            "100",
        ],
    );
    assert_eq!(
        result["data"]["jpeg_background"],
        serde_json::json!([0, 0, 255, 255])
    );
    assert!(
        result["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["code"] == "alpha_flattened")
    );
    let output = image::open(dir.path().join("flat.jpg"))
        .unwrap()
        .into_rgb8();
    assert!(
        output
            .pixels()
            .all(|p| p[0].abs_diff(188) <= 3 && p[1] <= 3 && p[2].abs_diff(187) <= 3)
    );
    failure(
        dir.path(),
        &[
            "identity",
            "--input",
            "alpha.png",
            "--output",
            "flat.jpg",
            "--overwrite",
        ],
        "alpha_not_supported",
    );
    assert_eq!(
        image::open(dir.path().join("flat.jpg"))
            .unwrap()
            .into_rgb8(),
        output
    );
}

#[test]
fn geometry_errors_are_structured_and_never_replace_outputs() {
    use serde_json::json;
    let (dir, _) = fixture();
    fs::write(dir.path().join("keep.png"), b"keep original bytes").unwrap();
    for (op, params, code) in [
        (
            "crop",
            json!({"x":15,"y":0,"width":2,"height":1}),
            "invalid_argument",
        ),
        (
            "crop",
            json!({"x":4294967295u32,"y":0,"width":1,"height":1}),
            "invalid_argument",
        ),
        (
            "resize",
            json!({"width":0,"filter":"nearest"}),
            "invalid_argument",
        ),
        (
            "resize",
            json!({"width":4294967296u64,"filter":"nearest"}),
            "invalid_argument",
        ),
        (
            "resize",
            json!({"width":32769,"filter":"nearest"}),
            "resource_limit",
        ),
        (
            "canvas",
            json!({"width":32768,"height":32768,"anchor":"center","background":[0,0,0,0]}),
            "resource_limit",
        ),
    ] {
        write_pipeline(
            dir.path(),
            vec![
                geometry_op("flip", json!({"axis":"horizontal"})),
                geometry_op(op, params),
            ],
        );
        failure(
            dir.path(),
            &[
                "run",
                "--input",
                "input.png",
                "--pipeline",
                "geometry.json",
                "--output",
                "keep.png",
                "--overwrite",
            ],
            code,
        );
        assert_eq!(
            fs::read(dir.path().join("keep.png")).unwrap(),
            b"keep original bytes"
        );
        assert_no_temporaries(dir.path());
    }
    for arguments in [
        vec!["resize", "--width", "-1"],
        vec!["resize", "--width", "4294967296"],
        vec!["resize", "--width", "1", "--filter", "cubic"],
        vec!["rotate", "--degrees", "NaN"],
        vec!["rotate", "--degrees", "inf"],
        vec!["rotate", "--degrees", "-361"],
        vec![
            "canvas",
            "--width",
            "2",
            "--height",
            "2",
            "--background",
            "#abcdefgg",
        ],
        vec!["identity", "--png-compression", "10"],
        vec!["identity", "--jpeg-background", "#ff0000"],
    ] {
        let mut args = arguments;
        args.extend([
            "--input",
            "input.png",
            "--output",
            "keep.png",
            "--overwrite",
        ]);
        failure(dir.path(), &args, "invalid_argument");
        assert_eq!(
            fs::read(dir.path().join("keep.png")).unwrap(),
            b"keep original bytes"
        );
    }
}
