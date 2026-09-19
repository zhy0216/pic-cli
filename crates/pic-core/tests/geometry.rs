use pic_core::{
    ErrorCode,
    codec::{self, EncodeOptions, Format},
    document::Raster,
    limits::ResourceLimits,
    operation::{OperationSpec, geometry::ResizeParams},
    pipeline::Pipeline,
};
use serde_json::{Value, json};

fn spec(op: &str, params: Value) -> OperationSpec {
    serde_json::from_value(json!({"op":op,"op_version":1,"target":"canvas","params":params}))
        .unwrap()
}

fn apply(
    raster: Raster,
    steps: Vec<OperationSpec>,
) -> pic_core::Result<pic_core::pipeline::Execution> {
    let dir = tempfile::tempdir().unwrap();
    let pipeline = json!({"schema_version":1,"operations":steps});
    Pipeline::from_json(
        &serde_json::to_vec(&pipeline).unwrap(),
        dir.path(),
        &ResourceLimits::default(),
    )?
    .execute(raster)
}

fn labeled() -> Raster {
    Raster::from_linear_rgba(
        3,
        2,
        (0..6)
            .map(|n| [n as f32 + 0.123_456_79, -0.0, -7.125, n as f32 / 5.0])
            .collect(),
        &ResourceLimits::default(),
    )
    .unwrap()
}

fn rotate(degrees: f64, expand: bool, filter: &str) -> OperationSpec {
    spec(
        "rotate",
        json!({"degrees":degrees,"expand":expand,"filter":filter,"background":[0,0,0,0]}),
    )
}

fn assert_labels(actual: &Raster, expected: &[usize]) {
    let original = labeled();
    assert_eq!(actual.pixels().len(), expected.len());
    for (pixel, &label) in actual.pixels().iter().zip(expected) {
        assert_eq!(
            pixel.map(f32::to_bits),
            original.pixels()[label].map(f32::to_bits)
        );
    }
}

#[test]
fn orthogonal_geometry_copies_coordinates_and_every_float_bit() {
    for (operation, size, labels) in [
        (
            spec("crop", json!({"x":1,"y":0,"width":2,"height":2})),
            (2, 2),
            vec![1, 2, 4, 5],
        ),
        (
            spec("flip", json!({"axis":"horizontal"})),
            (3, 2),
            vec![2, 1, 0, 5, 4, 3],
        ),
        (
            spec("flip", json!({"axis":"vertical"})),
            (3, 2),
            vec![3, 4, 5, 0, 1, 2],
        ),
        (
            rotate(90.0, true, "bilinear"),
            (2, 3),
            vec![3, 0, 4, 1, 5, 2],
        ),
        (
            rotate(-90.0, true, "nearest"),
            (2, 3),
            vec![2, 5, 1, 4, 0, 3],
        ),
        (
            rotate(180.0, false, "bilinear"),
            (3, 2),
            vec![5, 4, 3, 2, 1, 0],
        ),
        (
            rotate(360.0, true, "bilinear"),
            (3, 2),
            vec![0, 1, 2, 3, 4, 5],
        ),
        (
            spec("resize", json!({"width":6,"height":2,"filter":"nearest"})),
            (6, 2),
            vec![0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5],
        ),
    ] {
        let output = apply(labeled(), vec![operation]).unwrap().raster;
        assert_eq!((output.width(), output.height()), size);
        assert_labels(&output, &labels);
    }
    let input = labeled();
    let output = apply(
        input.clone(),
        vec![
            rotate(-360.0, true, "bilinear"),
            spec("resize", json!({"width":3,"height":2,"filter":"bilinear"})),
        ],
    )
    .unwrap();
    assert_eq!(input.pixels().as_ptr(), output.raster.pixels().as_ptr());
}

#[test]
fn pipeline_coordinates_follow_each_current_canvas_and_keep_logical_steps() {
    let result = apply(
        labeled(),
        vec![
            spec("crop", json!({"x":1,"y":0,"width":2,"height":2})),
            rotate(90.0, true, "bilinear"),
            spec("flip", json!({"axis":"horizontal"})),
        ],
    )
    .unwrap();
    assert_labels(&result.raster, &[1, 4, 2, 5]);
    assert_eq!(
        result.steps.iter().map(|s| s.index).collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(result.steps[1].params["expand"], true);
    let error = apply(
        labeled(),
        vec![
            spec("resize", json!({"width":1,"height":1,"filter":"nearest"})),
            spec("crop", json!({"x":1,"y":0,"width":1,"height":1})),
        ],
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert!(error.message.starts_with("operations[1]:"));
}

#[test]
fn canvas_nine_anchors_padding_clipping_and_odd_center_offsets() {
    let original = [0.25, 2.123_456_7, -0.0, 0.0];
    for (anchor, x, y) in [
        ("top_left", 0, 0),
        ("top", 1, 0),
        ("top_right", 2, 0),
        ("left", 0, 1),
        ("center", 1, 1),
        ("right", 2, 1),
        ("bottom_left", 0, 2),
        ("bottom", 1, 2),
        ("bottom_right", 2, 2),
    ] {
        let input =
            Raster::from_linear_rgba(1, 1, vec![original], &ResourceLimits::default()).unwrap();
        let result = apply(
            input,
            vec![spec(
                "canvas",
                json!({"width":3,"height":3,"anchor":anchor,"background":[255,0,0,128]}),
            )],
        )
        .unwrap();
        for (index, pixel) in result.raster.pixels().iter().enumerate() {
            let expected = if index == y * 3 + x {
                original
            } else {
                [1.0, 0.0, 0.0, 128.0 / 255.0]
            };
            assert_eq!(
                pixel.map(f32::to_bits),
                expected.map(f32::to_bits),
                "{anchor}"
            );
        }
    }
    let result = apply(
        labeled(),
        vec![spec(
            "canvas",
            json!({"width":2,"height":1,"anchor":"center","background":[0,0,0,0]}),
        )],
    )
    .unwrap();
    assert_labels(&result.raster, &[4, 5]);
    let result = apply(
        labeled(),
        vec![spec(
            "canvas",
            json!({"width":4,"height":3,"anchor":"center","background":[0,0,255,0]}),
        )],
    )
    .unwrap();
    assert_eq!(result.raster.pixels()[3], [0.0, 0.0, 1.0, 0.0]);
    assert_eq!(result.raster.pixels()[0], labeled().pixels()[0]);
}

#[test]
fn bilinear_resize_is_premultiplied_linear_light_with_no_black_or_hidden_color_fringe() {
    let limits = ResourceLimits::default();
    let input = Raster::from_linear_rgba(
        2,
        1,
        vec![[1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 0.0]],
        &limits,
    )
    .unwrap();
    let result = apply(
        input,
        vec![spec(
            "resize",
            json!({"width":4,"height":1,"filter":"bilinear"}),
        )],
    )
    .unwrap();
    for (pixel, alpha) in result.raster.pixels().iter().zip([1.0, 0.75, 0.25, 0.0]) {
        assert!((pixel[3] - alpha).abs() < 1e-6);
        assert_eq!(pixel[1], 0.0);
        assert_eq!(pixel[2], 0.0);
        assert_eq!(pixel[0], if alpha == 0.0 { 0.0 } else { 1.0 });
    }
    let input = Raster::from_linear_rgba(
        2,
        1,
        vec![[0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0]],
        &limits,
    )
    .unwrap();
    let result = apply(
        input,
        vec![spec(
            "resize",
            json!({"width":1,"height":1,"filter":"bilinear"}),
        )],
    )
    .unwrap();
    assert_eq!(result.raster.pixels()[0], [0.5, 0.5, 0.5, 1.0]);
    let encoded = codec::encode(
        &result.raster,
        &EncodeOptions::default()
            .resolve(std::path::Path::new("out.png"))
            .unwrap(),
        &limits,
    )
    .unwrap();
    assert_eq!(
        image::load_from_memory(&encoded)
            .unwrap()
            .into_rgba8()
            .get_pixel(0, 0)
            .0,
        [188, 188, 188, 255]
    );
    let input =
        Raster::from_linear_rgba(2, 1, vec![[4.123_456, -2.125, 0.0, 0.5]; 2], &limits).unwrap();
    let result = apply(
        input,
        vec![spec(
            "resize",
            json!({"width":1,"height":3,"filter":"bilinear"}),
        )],
    )
    .unwrap();
    assert!(
        result
            .raster
            .pixels()
            .iter()
            .all(|p| (p[0] - 4.123_456).abs() < 1e-6 && (p[1] + 2.125).abs() < 1e-6)
    );
    // Both convolution axes: hidden blue samples contribute no color or coverage.
    let input = Raster::from_linear_rgba(
        2,
        2,
        vec![
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
        &limits,
    )
    .unwrap();
    let result = apply(
        input,
        vec![spec(
            "resize",
            json!({"width":1,"height":1,"filter":"bilinear"}),
        )],
    )
    .unwrap();
    assert_eq!(result.raster.pixels(), &[[1.0, 0.0, 0.0, 0.25]]);
}

#[test]
fn resize_aspect_ratio_rounding_minimum_dimension_and_overflow_are_explicit() {
    for (params, input, expected) in [
        (json!({"width":1,"filter":"nearest"}), (2, 3), (1, 2)),
        (json!({"height":1,"filter":"nearest"}), (3, 2), (2, 1)),
        (json!({"width":1,"filter":"nearest"}), (100, 1), (1, 1)),
        (
            json!({"width":3,"height":9,"filter":"nearest"}),
            (2, 3),
            (3, 9),
        ),
    ] {
        let p: ResizeParams = serde_json::from_value(params).unwrap();
        assert_eq!(p.dimensions(input.0, input.1).unwrap(), expected);
    }
    let p: ResizeParams =
        serde_json::from_value(json!({"width":4294967295u32,"filter":"nearest"})).unwrap();
    assert_eq!(
        p.dimensions(1, 2).unwrap_err().code,
        ErrorCode::ResourceLimit
    );
}

#[test]
fn arbitrary_rotation_has_defined_bounds_center_and_alpha_sampling() {
    let input =
        Raster::from_linear_rgba(1, 1, vec![[1.0, 0.0, 0.0, 1.0]], &ResourceLimits::default())
            .unwrap();
    let result = apply(input.clone(), vec![rotate(45.0, true, "bilinear")]).unwrap();
    assert_eq!((result.raster.width(), result.raster.height()), (2, 2));
    for pixel in result.raster.pixels() {
        assert_eq!(&pixel[..3], &[1.0, 0.0, 0.0]);
        assert!((pixel[3] - (1.0 - std::f32::consts::FRAC_1_SQRT_2)).abs() < 1e-6);
    }
    let result = apply(input, vec![rotate(37.0, false, "nearest")]).unwrap();
    assert_eq!(result.raster.pixels(), &[[1.0, 0.0, 0.0, 1.0]]);
    let result = apply(labeled(), vec![rotate(45.0, true, "nearest")]).unwrap();
    assert_eq!((result.raster.width(), result.raster.height()), (4, 4));
    assert_eq!(result.raster.pixels()[0], [0.0; 4]);
    let result = apply(labeled(), vec![rotate(90.0, false, "nearest")]).unwrap();
    assert_eq!((result.raster.width(), result.raster.height()), (3, 2));
    // Non-square fixed canvas: ties select right/bottom; the left column is outside.
    assert_eq!(result.raster.pixels()[0], [0.0; 4]);
    assert_eq!(result.raster.pixels()[1], labeled().pixels()[4]);
    assert_eq!(result.raster.pixels()[2], labeled().pixels()[1]);
    let mut rotation = rotate(45.0, true, "bilinear");
    rotation.params["background"] = json!([0, 255, 0, 255]);
    let result = apply(labeled(), vec![rotation]).unwrap();
    assert_eq!(result.raster.pixels()[0], [0.0, 1.0, 0.0, 1.0]);
}

#[test]
fn geometry_rejects_unknown_missing_nonfinite_or_out_of_range_parameters() {
    for (op, params) in [
        (
            "crop",
            json!({"x":4294967295u32,"y":0,"width":1,"height":1}),
        ),
        ("crop", json!({"x":-1,"y":0,"width":1,"height":1})),
        ("resize", json!({"filter":"nearest"})),
        ("resize", json!({"width":0,"filter":"nearest"})),
        ("resize", json!({"width":1,"filter":"lanczos3"})),
        ("resize", json!({"width":1})),
        ("resize", json!({"width":1,"filter":"nearest","ignored":1})),
        (
            "rotate",
            json!({"degrees":361,"expand":true,"filter":"nearest","background":[0,0,0,0]}),
        ),
        (
            "rotate",
            json!({"degrees":null,"expand":true,"filter":"nearest","background":[0,0,0,0]}),
        ),
        (
            "rotate",
            json!({"degrees":45,"expand":true,"filter":"nearest","background":[0,0,0,256]}),
        ),
        ("flip", json!({"axis":"both"})),
        (
            "canvas",
            json!({"width":1,"height":1,"anchor":"middle","background":[0,0,0,0]}),
        ),
    ] {
        assert_eq!(
            spec(op, params).validate().unwrap_err().code,
            ErrorCode::InvalidArgument,
            "{op}"
        );
    }
    let mut operation = rotate(45.0, true, "nearest").validate().unwrap();
    if let pic_core::operation::Operation::Rotate(p) = &mut operation {
        p.degrees = f64::NAN;
    }
    assert_eq!(
        operation.validate().unwrap_err().code,
        ErrorCode::InvalidArgument
    );
}

#[test]
fn every_intermediate_and_resize_scratch_obeys_pipeline_limits() {
    let dir = tempfile::tempdir().unwrap();
    let limits = ResourceLimits {
        max_dimension: 4,
        ..ResourceLimits::default()
    };
    let pipeline = Pipeline::single(
        spec(
            "canvas",
            json!({"width":5,"height":1,"anchor":"top_left","background":[0,0,0,0]}),
        ),
        dir.path(),
        &limits,
    )
    .unwrap();
    assert_eq!(
        pipeline.execute(labeled()).unwrap_err().code,
        ErrorCode::ResourceLimit
    );
    // Both 64x1 and 1x64 fit individually; the cross-axis convolution scratch does not.
    let limits = ResourceLimits {
        max_buffer_bytes: 5000,
        ..ResourceLimits::default()
    };
    let input = Raster::from_linear_rgba(64, 1, vec![[1.0; 4]; 64], &limits).unwrap();
    let pipeline = Pipeline::single(
        spec("resize", json!({"width":1,"height":64,"filter":"bilinear"})),
        dir.path(),
        &limits,
    )
    .unwrap();
    assert_eq!(
        pipeline.execute(input).unwrap_err().code,
        ErrorCode::ResourceLimit
    );
    let pipeline = Pipeline::single(
        spec("resize", json!({"width":5,"height":1,"filter":"nearest"})),
        dir.path(),
        &ResourceLimits::default(),
    )
    .unwrap();
    assert_eq!(
        pipeline
            .execute_with_limits(
                labeled(),
                &ResourceLimits {
                    max_dimension: 4,
                    ..ResourceLimits::default()
                }
            )
            .unwrap_err()
            .code,
        ErrorCode::ResourceLimit
    );
}

#[test]
fn jpeg_flattening_uses_linear_background_without_mutating_working_alpha() {
    let limits = ResourceLimits::default();
    let input = Raster::from_linear_rgba(8, 8, vec![[1.0, 0.0, 0.0, 0.5]; 64], &limits).unwrap();
    let encoding = EncodeOptions {
        format: Some(Format::Jpeg),
        jpeg_quality: Some(100),
        jpeg_background: Some("#0000ff".parse().unwrap()),
        ..EncodeOptions::default()
    }
    .resolve(std::path::Path::new("out.jpg"))
    .unwrap();
    let bytes = codec::encode(&input, &encoding, &limits).unwrap();
    let output = image::load_from_memory(&bytes).unwrap().into_rgb8();
    for p in output.pixels() {
        assert!(
            p[0].abs_diff(188) <= 3 && p[1] <= 3 && p[2].abs_diff(188) <= 3,
            "{p:?}"
        );
    }
    assert!(input.pixels().iter().all(|p| p[3] == 0.5));
    for (format, compression, background) in [
        (Format::Jpeg, Some(6), None),
        (Format::Png, Some(10), None),
        (Format::Png, None, Some("#ffffff")),
        (Format::Jpeg, None, Some("#ffffff80")),
    ] {
        assert_eq!(
            EncodeOptions {
                format: Some(format),
                png_compression: compression,
                jpeg_background: background.map(|c| c.parse().unwrap()),
                ..EncodeOptions::default()
            }
            .resolve(std::path::Path::new("out"))
            .unwrap_err()
            .code,
            ErrorCode::InvalidArgument
        );
    }
}
