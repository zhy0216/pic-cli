use pic_core::{
    ErrorCode,
    document::Raster,
    limits::ResourceLimits,
    operation::{Operation, OperationSpec, adjustments::*},
    pipeline::{Pipeline, ResourceResolver},
};
use serde_json::{Value, json};

fn spec(op: &str, params: Value) -> OperationSpec {
    serde_json::from_value(json!({"op":op,"op_version":1,"target":"canvas","params":params}))
        .unwrap()
}

fn raster(pixels: &[[f32; 4]]) -> Raster {
    Raster::from_linear_rgba(
        pixels.len() as u32,
        1,
        pixels.to_vec(),
        &ResourceLimits::default(),
    )
    .unwrap()
}

fn apply(input: &Raster, steps: &[OperationSpec]) -> Raster {
    let dir = tempfile::tempdir().unwrap();
    Pipeline::from_json(
        &serde_json::to_vec(&json!({"schema_version":1,"operations":steps})).unwrap(),
        dir.path(),
        &ResourceLimits::default(),
    )
    .unwrap()
    .execute(input.clone())
    .unwrap()
    .raster
}

fn adjustment(exposure: f64, brightness: f64, contrast: f64, saturation: f64) -> OperationSpec {
    spec(
        "adjust",
        json!({"exposure":exposure,"brightness":brightness,"contrast":contrast,"saturation":saturation}),
    )
}

fn default_levels() -> Value {
    json!({"channel":"rgb","input_black":0,"input_white":1,"gamma":1,"output_black":0,"output_white":1})
}

fn close(actual: &Raster, expected: &[[f32; 4]]) {
    assert_eq!(actual.pixels().len(), expected.len());
    for (index, (actual, expected)) in actual.pixels().iter().zip(expected).enumerate() {
        for channel in 0..4 {
            assert!(
                (actual[channel] - expected[channel]).abs() <= 2e-6,
                "pixel {index}, channel {channel}: {actual:?} != {expected:?}"
            );
        }
    }
}

#[test]
fn neutral_parameters_preserve_every_float_bit_and_shared_storage() {
    let input = raster(&[
        [-0.0, -0.25, 2.0, -0.0],
        [f32::from_bits(1), 0.12345679, 1.2345678, f32::from_bits(1)],
        [f32::MAX, f32::MIN, 0.5, 1.0],
    ]);
    let steps = [
        adjustment(0.0, 0.0, 1.0, 1.0),
        spec("levels", default_levels()),
        spec(
            "levels",
            json!({"channel":"red","input_black":0.25,"input_white":0.75,"gamma":1,"output_black":0.25,"output_white":0.75}),
        ),
        spec(
            "curves",
            json!({"channel":"rgb","points":[[0,0],[0.25,0.25],[1,1]]}),
        ),
        spec("blur", json!({"sigma":0})),
        spec("sharpen", json!({"sigma":100,"amount":0})),
        spec("sharpen", json!({"sigma":0,"amount":10})),
    ];
    let output = apply(&input, &steps);
    assert_eq!(input.pixels().as_ptr(), output.pixels().as_ptr());
    for (a, b) in input.pixels().iter().zip(output.pixels()) {
        assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits));
    }
}

#[test]
fn adjustment_controls_have_independent_linear_rgb_expectations() {
    let input = raster(&[[0.25, 0.5, 0.75, 0.25], [-0.25, 1.5, 0.0, 0.0]]);
    close(
        &apply(&input, &[adjustment(1.0, 0.0, 1.0, 1.0)]),
        &[[0.5, 1.0, 1.5, 0.25], [-0.5, 3.0, 0.0, 0.0]],
    );
    close(
        &apply(&input, &[adjustment(-1.0, 0.0, 1.0, 1.0)]),
        &[[0.125, 0.25, 0.375, 0.25], [-0.125, 0.75, 0.0, 0.0]],
    );
    close(
        &apply(&input, &[adjustment(0.0, 0.25, 1.0, 1.0)]),
        &[[0.5, 0.75, 1.0, 0.25], [0.0, 1.75, 0.25, 0.0]],
    );
    close(
        &apply(&input, &[adjustment(0.0, 0.0, 2.0, 1.0)]),
        &[[0.0, 0.5, 1.0, 0.25], [-1.0, 2.5, -0.5, 0.0]],
    );
    close(
        &apply(&input, &[adjustment(0.0, 0.0, 0.0, 1.0)]),
        &[[0.5, 0.5, 0.5, 0.25], [0.5, 0.5, 0.5, 0.0]],
    );
    close(
        &apply(
            &raster(&[[1.0, 0.0, 0.0, 0.5]]),
            &[adjustment(0.0, 0.0, 1.0, 2.0)],
        ),
        &[[1.7874, -0.2126, -0.2126, 0.5]],
    );
    // EV -> offset -> contrast -> saturation: [0.25,0.5,0.75] becomes [0,1,2], then Y.
    close(
        &apply(
            &raster(&[[0.25, 0.5, 0.75, 0.25]]),
            &[adjustment(1.0, -0.25, 2.0, 0.0)],
        ),
        &[[0.8596, 0.8596, 0.8596, 0.25]],
    );
}

#[test]
fn grayscale_and_invert_transform_hidden_rgb_but_preserve_alpha_bits() {
    let input = raster(&[
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.25],
        [0.0, 0.0, 1.0, -0.0],
    ]);
    let expected = [[0.2126; 3], [0.7152; 3], [0.0722; 3]];
    let gray = apply(&input, &[spec("grayscale", json!({}))]);
    let saturation = apply(&input, &[adjustment(0.0, 0.0, 1.0, 0.0)]);
    assert_eq!(gray.pixels(), saturation.pixels());
    for (index, (before, after)) in input.pixels().iter().zip(gray.pixels()).enumerate() {
        for channel in 0..3 {
            assert!((after[channel] - expected[index][channel]).abs() < 1e-7);
        }
        assert_eq!(before[3].to_bits(), after[3].to_bits());
    }
    let input = raster(&[[0.0, 0.25, 1.0, -0.0], [-0.5, 1.5, 0.125, 0.5]]);
    let inverted = apply(&input, &[spec("invert", json!({}))]);
    close(
        &inverted,
        &[[1.0, 0.75, 0.0, -0.0], [1.5, -0.5, 0.875, 0.5]],
    );
    assert_eq!(inverted.pixels()[0][3].to_bits(), (-0.0f32).to_bits());
    assert_eq!(
        apply(&inverted, &[spec("invert", json!({}))]).pixels(),
        input.pixels()
    );
}

#[test]
fn levels_map_black_white_gamma_and_signed_out_of_range_values() {
    let input = raster(&[[0.25, 0.375, 0.75, 0.5], [0.125, 1.375, 0.5, 0.0]]);
    let params = json!({"channel":"rgb","input_black":0.25,"input_white":0.75,"gamma":2,"output_black":0.125,"output_white":0.875});
    close(
        &apply(&input, &[spec("levels", params)]),
        &[[0.125, 0.5, 0.875, 0.5], [-0.25, 1.25, 0.6553301, 0.0]],
    );
    let mut params = default_levels();
    params["gamma"] = json!(0.5);
    close(
        &apply(
            &raster(&[[-0.5, 0.5, 2.0, 0.25]]),
            &[spec("levels", params)],
        ),
        &[[-0.25, 0.25, 4.0, 0.25]],
    );
    let params = json!({"channel":"rgb","input_black":0,"input_white":1e-300,"gamma":0.1,"output_black":0.25,"output_white":0.25});
    close(
        &apply(
            &raster(&[[f32::MAX, f32::MIN, 0.0, 0.0]]),
            &[spec("levels", params)],
        ),
        &[[0.25, 0.25, 0.25, 0.0]],
    );
}

#[test]
fn curves_hit_controls_interpolate_nonmonotonic_segments_and_extend_endpoints() {
    let samples = [-0.25, 0.0, 0.125, 0.25, 0.375, 0.5, 0.75, 1.0, 1.5];
    let input = raster(&samples.map(|x| [x, x, x, 0.25]));
    let output = apply(
        &input,
        &[spec(
            "curves",
            json!({"channel":"rgb","points":[[0,0],[0.25,0.5],[0.5,0.25],[1,1]]}),
        )],
    );
    let expected = [-0.5, 0.0, 0.25, 0.5, 0.375, 0.25, 0.625, 1.0, 1.75];
    close(&output, &expected.map(|x| [x, x, x, 0.25]));
    assert_eq!(output.pixels()[3][0].to_bits(), 0.5f32.to_bits());
    assert_eq!(output.pixels()[5][0].to_bits(), 0.25f32.to_bits());
    close(
        &apply(
            &input,
            &[spec(
                "curves",
                json!({"channel":"rgb","points":[[0,0.5],[1,0.5]]}),
            )],
        ),
        &[[0.5, 0.5, 0.5, 0.25]; 9],
    );
}

#[test]
fn selected_channels_leave_other_channels_and_alpha_bit_exact() {
    let input = raster(&[[0.25, -0.0, 0.75, -0.0], [0.5, -0.25, 1.5, 0.5]]);
    for (channel_index, channel) in ["red", "green", "blue"].iter().enumerate() {
        let mut levels = default_levels();
        levels["channel"] = json!(channel);
        levels["output_black"] = json!(0.25);
        for operation in [
            spec("levels", levels),
            spec(
                "curves",
                json!({"channel":channel,"points":[[0,0.25],[1,1]]}),
            ),
        ] {
            let output = apply(&input, &[operation]);
            for (before, after) in input.pixels().iter().zip(output.pixels()) {
                for index in 0..4 {
                    if index == channel_index {
                        assert_eq!(after[index], 0.25 + 0.75 * before[index]);
                    } else {
                        assert_eq!(before[index].to_bits(), after[index].to_bits());
                    }
                }
            }
        }
    }
}

// Independently tabulated sigma=1, radius=3 discrete Gaussian weights.
const TAIL: f32 = 0.30047485; // w(1)+w(2)+w(3)
const CENTER_SIDE: f32 = 0.6995251; // w(0)+w(1)+w(2)+w(3)

#[test]
fn gaussian_impulses_and_clamped_edges_have_known_one_and_two_dimensional_pixels() {
    let output = apply(
        &raster(&[[0.0, 0.0, 0.0, 1.0], [1.0; 4], [0.0, 0.0, 0.0, 1.0]]),
        &[spec("blur", json!({"sigma":1}))],
    );
    close(
        &output,
        &[
            [0.24203622, 0.24203622, 0.24203622, 1.0],
            [0.39905027, 0.39905027, 0.39905027, 1.0],
            [0.24203622, 0.24203622, 0.24203622, 1.0],
        ],
    );
    let output = apply(
        &raster(&[[0.0, 0.0, 0.0, 1.0], [1.0; 4]]),
        &[spec("blur", json!({"sigma":1}))],
    );
    close(
        &output,
        &[
            [TAIL, TAIL, TAIL, 1.0],
            [CENTER_SIDE, CENTER_SIDE, CENTER_SIDE, 1.0],
        ],
    );
    let input = Raster::from_linear_rgba(
        2,
        2,
        vec![
            [0.0, 0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0, 1.0],
            [1.0; 4],
        ],
        &ResourceLimits::default(),
    )
    .unwrap();
    close(
        &apply(&input, &[spec("blur", json!({"sigma":1}))]),
        &[
            TAIL * TAIL,
            TAIL * CENTER_SIDE,
            TAIL * CENTER_SIDE,
            CENTER_SIDE * CENTER_SIDE,
        ]
        .map(|x| [x, x, x, 1.0]),
    );
    for sigma in [f64::MIN_POSITIVE, 1e-300, 100.0] {
        close(
            &apply(
                &raster(&[[-0.25, 2.0, 0.125, 0.5]]),
                &[spec("blur", json!({"sigma":sigma}))],
            ),
            &[[-0.25, 2.0, 0.125, 0.5]],
        );
    }
}

#[test]
fn gaussian_uses_premultiplied_alpha_and_never_leaks_hidden_color() {
    let input = raster(&[[1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 0.0]]);
    close(
        &apply(&input, &[spec("blur", json!({"sigma":1}))]),
        &[[1.0, 0.0, 0.0, CENTER_SIDE], [1.0, 0.0, 0.0, TAIL]],
    );
    let input = raster(&[[1.0, 0.0, 0.0, 0.5], [0.0, 0.0, 1.0, 0.25]]);
    let a = 0.5 * CENTER_SIDE + 0.25 * TAIL;
    let b = 0.5 * TAIL + 0.25 * CENTER_SIDE;
    close(
        &apply(&input, &[spec("blur", json!({"sigma":1}))]),
        &[
            [0.5 * CENTER_SIDE / a, 0.0, 0.25 * TAIL / a, a],
            [0.5 * TAIL / b, 0.0, 0.25 * CENTER_SIDE / b, b],
        ],
    );
    let hidden = raster(&[[f32::MAX, 2.0, -1.0, 0.0], [-1.0, f32::MIN, 0.0, -0.0]]);
    close(
        &apply(&hidden, &[spec("blur", json!({"sigma":1}))]),
        &[[0.0; 4]; 2],
    );
    // Premultiplication uses f64, so small but representable coverage retains color.
    close(
        &apply(
            &raster(&[[1.0, 0.5, -0.5, f32::MIN_POSITIVE]]),
            &[spec("blur", json!({"sigma":1}))],
        ),
        &[[1.0, 0.5, -0.5, f32::MIN_POSITIVE]],
    );
}

#[test]
fn unsharp_mask_retains_overshoot_alpha_and_transparent_hidden_rgb() {
    let input = raster(&[[0.0, 0.0, 0.0, 1.0], [1.0; 4]]);
    close(
        &apply(&input, &[spec("sharpen", json!({"sigma":1,"amount":2}))]),
        &[
            [-2.0 * TAIL, -2.0 * TAIL, -2.0 * TAIL, 1.0],
            [1.0 + 2.0 * TAIL, 1.0 + 2.0 * TAIL, 1.0 + 2.0 * TAIL, 1.0],
        ],
    );
    let input = raster(&[[1.0, 0.0, 0.0, 0.25], [-0.0, 0.75, 1.0, -0.0]]);
    let output = apply(&input, &[spec("sharpen", json!({"sigma":1,"amount":10}))]);
    for (before, after) in input.pixels().iter().zip(output.pixels()) {
        assert_eq!(before.map(f32::to_bits), after.map(f32::to_bits));
    }
}

#[test]
fn ordered_steps_retain_hdr_precision_and_each_logical_boundary() {
    let input = raster(&[[0.25, 0.5, 0.75, 0.5]]);
    let exposure = adjustment(1.0, 0.0, 1.0, 1.0);
    let brightness = adjustment(0.0, 0.25, 1.0, 1.0);
    close(
        &apply(&input, &[exposure.clone(), brightness.clone()]),
        &[[0.75, 1.25, 1.75, 0.5]],
    );
    close(
        &apply(&input, &[brightness, exposure]),
        &[[1.0, 1.5, 2.0, 0.5]],
    );
    let dir = tempfile::tempdir().unwrap();
    let steps = [
        adjustment(20.0, 0.0, 1.0, 1.0),
        adjustment(-20.0, 0.0, 1.0, 1.0),
    ];
    let pipeline = Pipeline::from_json(
        &serde_json::to_vec(&json!({"schema_version":1,"operations":steps})).unwrap(),
        dir.path(),
        &ResourceLimits::default(),
    )
    .unwrap();
    let execution = pipeline.execute(input.clone()).unwrap();
    assert_eq!(execution.raster.pixels(), input.pixels());
    assert_eq!(execution.steps.len(), 2);
    for (index, step) in execution.steps.iter().enumerate() {
        assert_eq!(step.index, index);
        assert_eq!(step.op, "adjust");
        assert_eq!(step.params, steps[index].params);
    }
}

#[test]
fn parameter_validation_is_strict_for_fields_ranges_channels_and_curve_shapes() {
    let valid = [
        adjustment(0.0, 0.0, 1.0, 1.0),
        spec("levels", default_levels()),
        spec("curves", json!({"channel":"rgb","points":[[0,0],[1,1]]})),
        spec("blur", json!({"sigma":1})),
        spec("sharpen", json!({"sigma":1,"amount":1})),
    ];
    for operation in valid {
        operation.validate().unwrap();
        for field in operation.params.as_object().unwrap().keys() {
            let mut missing = operation.clone();
            missing.params.as_object_mut().unwrap().remove(field);
            assert_eq!(
                missing.validate().unwrap_err().code,
                ErrorCode::InvalidArgument
            );
            for value in [Value::Null, json!(false)] {
                let mut invalid = operation.clone();
                invalid.params[field] = value;
                assert_eq!(
                    invalid.validate().unwrap_err().code,
                    ErrorCode::InvalidArgument
                );
            }
        }
        let mut unknown = operation.clone();
        unknown.params["unsupported"] = json!(0);
        assert_eq!(
            unknown.validate().unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }
    for (op, params) in [
        ("adjust", json!([0, 0, 1, 1])),
        ("levels", json!(["rgb", 0, 1, 1, 0, 1])),
        ("curves", json!(["rgb", [[0, 0], [1, 1]]])),
        ("blur", json!([1])),
        ("sharpen", json!([1, 1])),
        (
            "adjust",
            json!({"exposure":21,"brightness":0,"contrast":1,"saturation":1}),
        ),
        (
            "adjust",
            json!({"exposure":0,"brightness":-1.01,"contrast":1,"saturation":1}),
        ),
        (
            "adjust",
            json!({"exposure":0,"brightness":0,"contrast":-0.1,"saturation":1}),
        ),
        (
            "adjust",
            json!({"exposure":0,"brightness":0,"contrast":1,"saturation":11}),
        ),
        ("blur", json!({"sigma":-0.1})),
        ("blur", json!({"sigma":100.1})),
        ("sharpen", json!({"sigma":1,"amount":-1})),
        ("sharpen", json!({"sigma":1,"amount":11})),
        ("grayscale", json!({"channel":"alpha"})),
        ("invert", json!({"color_space":"srgb"})),
        ("curves", json!({"channel":"alpha","points":[[0,0],[1,1]]})),
    ] {
        assert_eq!(
            spec(op, params).validate().unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }
    for (key, value) in [
        ("input_black", json!(1)),
        ("input_white", json!(0)),
        ("output_black", json!(1.1)),
        ("output_white", json!(-1)),
        ("gamma", json!(0.09)),
        ("gamma", json!(10.1)),
        ("channel", json!("alpha")),
    ] {
        let mut params = default_levels();
        params[key] = value;
        assert_eq!(
            spec("levels", params).validate().unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }
    for points in [
        json!([[0, 0]]),
        json!([[0, 0], [0.5, 0.5], [0.5, 0.75], [1, 1]]),
        json!([[0, 0], [0.75, 0.5], [0.5, 0.75], [1, 1]]),
        json!([[0.1, 0], [1, 1]]),
        json!([[0, 0], [0.9, 1]]),
        json!([[0, 0], [1, 1.1]]),
        json!([[0, 0, 0], [1, 1]]),
        json!(vec![[0.0, 0.0]; 257]),
    ] {
        assert_eq!(
            spec("curves", json!({"channel":"rgb","points":points}))
                .validate()
                .unwrap_err()
                .code,
            ErrorCode::InvalidArgument
        );
    }
}

#[test]
fn typed_parameters_reject_all_nonfinite_values_before_serialization() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for field in 0..4 {
            let mut values = [0.0, 0.0, 1.0, 1.0];
            values[field] = bad;
            let [exposure, brightness, contrast, saturation] = values;
            assert!(
                AdjustParams {
                    exposure,
                    brightness,
                    contrast,
                    saturation
                }
                .validate()
                .is_err()
            );
        }
        for field in 0..5 {
            let mut values = [0.0, 1.0, 1.0, 0.0, 1.0];
            values[field] = bad;
            let [input_black, input_white, gamma, output_black, output_white] = values;
            assert!(
                LevelsParams {
                    channel: Channel::Rgb,
                    input_black,
                    input_white,
                    gamma,
                    output_black,
                    output_white
                }
                .validate()
                .is_err()
            );
        }
        for field in 0..2 {
            let mut points = vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]];
            points[1][field] = bad;
            assert!(
                CurvesParams {
                    channel: Channel::Rgb,
                    points
                }
                .validate()
                .is_err()
            );
        }
        assert!(BlurParams { sigma: bad }.validate().is_err());
        assert!(
            SharpenParams {
                sigma: bad,
                amount: 1.0
            }
            .validate()
            .is_err()
        );
        assert!(
            SharpenParams {
                sigma: 1.0,
                amount: bad
            }
            .validate()
            .is_err()
        );
    }
}

#[test]
fn buffer_limits_and_nonfinite_results_fail_without_replacing_the_raster() {
    let dir = tempfile::tempdir().unwrap();
    let resources = ResourceResolver::new(dir.path()).unwrap();
    let original = raster(&[[f32::MAX, f32::MIN, 0.5, 1.0], [0.0, 0.0, 0.0, 1.0]]);
    for operation in [
        adjustment(20.0, 0.0, 1.0, 1.0).validate().unwrap(),
        Operation::Levels(LevelsParams {
            channel: Channel::Rgb,
            input_black: 0.0,
            input_white: 1.0,
            gamma: 0.1,
            output_black: 0.0,
            output_white: 1.0,
        }),
        spec(
            "curves",
            json!({"channel":"rgb","points":[[0,0],[0.5,1],[1,0]]}),
        )
        .validate()
        .unwrap(),
        spec("sharpen", json!({"sigma":1,"amount":10}))
            .validate()
            .unwrap(),
    ] {
        let mut input = original.clone();
        assert_eq!(
            operation.apply(&mut input, &resources).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(input.pixels().as_ptr(), original.pixels().as_ptr());
    }
    let small = ResourceLimits {
        max_buffer_bytes: 128,
        ..ResourceLimits::default()
    };
    for operation in [
        spec("blur", json!({"sigma":1})),
        spec("sharpen", json!({"sigma":1,"amount":1})),
    ] {
        let pipeline = Pipeline::single(operation.clone(), dir.path(), &small).unwrap();
        assert_eq!(
            pipeline.execute(original.clone()).unwrap_err().code,
            ErrorCode::ResourceLimit
        );
        let pipeline = Pipeline::single(operation, dir.path(), &ResourceLimits::default()).unwrap();
        assert_eq!(
            pipeline
                .execute_with_limits(original.clone(), &small)
                .unwrap_err()
                .code,
            ErrorCode::ResourceLimit
        );
    }
}
