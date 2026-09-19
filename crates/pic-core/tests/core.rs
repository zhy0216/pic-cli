use std::{
    fs,
    path::Path,
    sync::{Arc, Barrier},
};

use image::{ImageEncoder, Rgba, RgbaImage};
use pic_core::{
    ErrorCode,
    codec::{self, EncodeOptions, Format},
    document::{PIXEL_SEMANTICS, Raster},
    limits::ResourceLimits,
    operation::OperationSpec,
    pipeline::{Pipeline, PipelineSpec, ResourceResolver},
    result::Diagnostics,
};

fn pipeline(operations: Vec<OperationSpec>, base: &Path) -> Pipeline {
    Pipeline::new(
        PipelineSpec {
            schema_version: 1,
            operations,
        },
        base,
        &ResourceLimits::default(),
    )
    .unwrap()
}

#[test]
fn identity_steps_preserve_float_bits_and_shared_storage() {
    let limits = ResourceLimits::default();
    let pixels = vec![
        [0.123_456_79, -0.25, 7.125, 0.0],
        [-0.0, 0.000_031_7, 1.5, 0.543_219],
    ];
    let raster = Raster::from_linear_rgba(2, 1, pixels.clone(), &limits).unwrap();
    let original = raster.clone();
    let dir = tempfile::tempdir().unwrap();
    let output = pipeline(vec![OperationSpec::identity(); 3], dir.path())
        .execute(raster)
        .unwrap();
    assert_eq!(
        output
            .steps
            .iter()
            .map(|step| step.index)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert!(
        output
            .steps
            .iter()
            .all(|step| step.op == "identity" && step.op_version == 1 && step.target.0 == "canvas")
    );
    for (actual, expected) in output.raster.pixels().iter().zip(pixels) {
        assert_eq!(actual.map(f32::to_bits), expected.map(f32::to_bits));
    }
    assert_eq!(original.pixels().as_ptr(), output.raster.pixels().as_ptr());
    let empty = pipeline(vec![], dir.path()).execute(output.raster).unwrap();
    assert!(empty.steps.is_empty());
    assert_eq!(original.pixels().as_ptr(), empty.raster.pixels().as_ptr());
    assert_eq!(PIXEL_SEMANTICS, "linear_srgb_rgba32f_straight_v1");
}

#[test]
fn raster_rejects_invalid_samples_and_resource_sizes() {
    let limits = ResourceLimits::default();
    for pixel in [
        [f32::NAN, 0.0, 0.0, 1.0],
        [0.0, f32::INFINITY, 0.0, 1.0],
        [0.0, 0.0, 0.0, -0.1],
        [0.0, 0.0, 0.0, 1.01],
    ] {
        assert_eq!(
            Raster::from_linear_rgba(1, 1, vec![pixel], &limits)
                .unwrap_err()
                .code,
            ErrorCode::InvalidArgument
        );
    }
    assert_eq!(
        Raster::from_linear_rgba(0, 1, vec![], &limits)
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        Raster::from_linear_rgba(1, 1, vec![], &limits)
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        limits
            .check_dimensions(u32::MAX, u32::MAX)
            .unwrap_err()
            .code,
        ErrorCode::ResourceLimit
    );
    let small = ResourceLimits {
        max_buffer_bytes: 31,
        ..limits
    };
    assert_eq!(
        small.check_dimensions(1, 1).unwrap_err().code,
        ErrorCode::ResourceLimit
    );
}

#[test]
fn all_srgb_bytes_and_hidden_transparent_colors_survive_png_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.png");
    let original = RgbaImage::from_fn(256, 2, |x, y| {
        Rgba([
            x as u8,
            (255 - x) as u8,
            127,
            if y == 0 { 0 } else { x as u8 },
        ])
    });
    original.save(&input).unwrap();
    let mut diagnostics = Diagnostics::default();
    let limits = ResourceLimits::default();
    let loaded = codec::load(&input, &limits, &mut diagnostics).unwrap();
    assert!(loaded.info.has_alpha && loaded.info.has_transparency);
    let execution = pipeline(vec![OperationSpec::identity(); 4], dir.path())
        .execute(loaded.raster)
        .unwrap();
    let encoding = EncodeOptions::default()
        .resolve(Path::new("result.png"))
        .unwrap();
    let encoded = codec::encode(&execution.raster, &encoding, &limits).unwrap();
    assert_eq!(
        image::load_from_memory(&encoded).unwrap().into_rgba8(),
        original
    );
    assert!(diagnostics.timings.decode_ms > 0.0);
    assert_eq!(diagnostics.warnings[0].code, "assumed_srgb");
    let jpeg = EncodeOptions {
        format: Some(Format::Jpeg),
        jpeg_quality: None,
        ..EncodeOptions::default()
    }
    .resolve(Path::new("result.jpg"))
    .unwrap();
    assert_eq!(
        codec::encode(&execution.raster, &jpeg, &limits)
            .unwrap_err()
            .code,
        ErrorCode::AlphaNotSupported
    );
}

#[test]
fn strict_pipeline_validation_rejects_versions_unknown_ops_and_parameters() {
    let dir = tempfile::tempdir().unwrap();
    let limits = ResourceLimits::default();
    let cases = [
        (
            r#"{"schema_version":2,"operations":[]}"#,
            ErrorCode::UnsupportedVersion,
        ),
        (
            r#"{"schema_version":1,"operations":[{"op":"future-op","op_version":1,"target":"canvas","params":{}}]}"#,
            ErrorCode::UnknownOperation,
        ),
        (
            r#"{"schema_version":1,"operations":[{"op":"identity","op_version":2,"target":"canvas","params":{}}]}"#,
            ErrorCode::UnsupportedVersion,
        ),
        (
            r#"{"schema_version":1,"operations":[{"op":"identity","op_version":1,"target":"invalid layer ID","params":{}}]}"#,
            ErrorCode::InvalidTarget,
        ),
        (
            r#"{"schema_version":1,"operations":[{"op":"identity","op_version":1,"target":"canvas","params":{"exposure":1}}]}"#,
            ErrorCode::InvalidArgument,
        ),
        (
            r#"{"schema_version":1,"operations":[{"op":"identity","op_version":1,"target":"canvas","params":null}]}"#,
            ErrorCode::InvalidArgument,
        ),
        (
            r#"{"schema_version":1,"operations":[],"ignored":true}"#,
            ErrorCode::InvalidJson,
        ),
        (
            r#"{"schema_version":1,"schema_version":1,"operations":[]}"#,
            ErrorCode::InvalidJson,
        ),
        (
            r#"{"schema_version":1,"operations":[{"op":"identity"}]}"#,
            ErrorCode::InvalidJson,
        ),
        (
            r#"{"schema_version":1,"operations":[]} trailing"#,
            ErrorCode::InvalidJson,
        ),
    ];
    for (json, code) in cases {
        assert_eq!(
            Pipeline::from_json(json.as_bytes(), dir.path(), &limits)
                .unwrap_err()
                .code,
            code,
            "{json}"
        );
    }
    let mut bad = OperationSpec::identity();
    bad.op = "future-op".into();
    let spec = PipelineSpec {
        schema_version: 1,
        operations: vec![OperationSpec::identity(), bad],
    };
    let error = Pipeline::new(spec, dir.path(), &limits).unwrap_err();
    assert!(error.message.starts_with("operations[1]:"));
}

#[test]
fn resources_resolve_from_pipeline_directory_not_the_process_directory() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("pipelines with spaces");
    fs::create_dir(&nested).unwrap();
    let file = nested.join("pipeline.json");
    fs::write(&file, r#"{"schema_version":1,"operations":[]}"#).unwrap();
    let pipeline = Pipeline::from_file(&file, &ResourceLimits::default()).unwrap();
    assert_eq!(
        pipeline
            .resources()
            .resolve(Path::new("assets/input.png"))
            .unwrap(),
        nested.join("assets/input.png")
    );
    assert_eq!(pipeline.resources().resolve(&file).unwrap(), file);
    assert!(pipeline.resources().resolve(Path::new("")).is_err());
    assert!(ResourceResolver::new(&file).is_err());
}

#[test]
fn admission_limits_apply_before_processing() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("input.png");
    RgbaImage::from_pixel(2, 2, Rgba([0, 0, 0, 255]))
        .save(&file)
        .unwrap();
    for limits in [
        ResourceLimits {
            max_pixels: 3,
            ..ResourceLimits::default()
        },
        ResourceLimits {
            max_dimension: 1,
            ..ResourceLimits::default()
        },
        ResourceLimits {
            max_input_bytes: 8,
            ..ResourceLimits::default()
        },
    ] {
        assert_eq!(
            codec::load(&file, &limits, &mut Diagnostics::default())
                .err()
                .unwrap()
                .code,
            ErrorCode::ResourceLimit
        );
    }
    let limits = ResourceLimits {
        max_operations: 0,
        ..ResourceLimits::default()
    };
    assert_eq!(
        Pipeline::single(OperationSpec::identity(), dir.path(), &limits)
            .unwrap_err()
            .code,
        ErrorCode::ResourceLimit
    );
    let limits = ResourceLimits {
        max_pipeline_bytes: 1,
        ..ResourceLimits::default()
    };
    assert_eq!(
        Pipeline::from_json(b"{}", dir.path(), &limits)
            .unwrap_err()
            .code,
        ErrorCode::ResourceLimit
    );
}

#[test]
fn sixteen_bit_png_is_rejected_without_quantization() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sixteen.png");
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            &[1, 0, 2, 0, 3, 0, 255, 255],
            1,
            1,
            image::ExtendedColorType::Rgba16,
        )
        .unwrap();
    fs::write(&path, bytes).unwrap();
    assert_eq!(
        codec::load(
            &path,
            &ResourceLimits::default(),
            &mut Diagnostics::default()
        )
        .err()
        .unwrap()
        .code,
        ErrorCode::UnsupportedColor
    );
}

#[test]
fn only_one_concurrent_no_clobber_publication_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("output");
    let barrier = Arc::new(Barrier::new(2));
    let threads = [b"complete-first".to_vec(), b"complete-second".to_vec()].map(|bytes| {
        let path = path.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            codec::publish(&path, &bytes, false)
        })
    });
    let outcomes = threads.map(|thread| thread.join().unwrap());
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes.into_iter().find_map(Result::err).unwrap().code,
        ErrorCode::OutputExists
    );
    let output = fs::read(&path).unwrap();
    assert!(output == b"complete-first" || output == b"complete-second");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    codec::publish(&path, b"replacement", true).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"replacement");
}

#[test]
fn encoding_options_do_not_ignore_invalid_parameters() {
    assert_eq!(
        EncodeOptions {
            format: None,
            jpeg_quality: Some(90),
            ..EncodeOptions::default()
        }
        .resolve(Path::new("out.png"))
        .unwrap_err()
        .code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        EncodeOptions {
            format: None,
            jpeg_quality: Some(0),
            ..EncodeOptions::default()
        }
        .resolve(Path::new("out.jpg"))
        .unwrap_err()
        .code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        EncodeOptions::default()
            .resolve(Path::new("out.webp"))
            .unwrap_err()
            .code,
        ErrorCode::UnsupportedFormat
    );
    assert_eq!(
        EncodeOptions {
            format: Some(Format::Png),
            jpeg_quality: None,
            ..EncodeOptions::default()
        }
        .resolve(Path::new("out.data"))
        .unwrap()
        .format,
        Format::Png
    );
}
