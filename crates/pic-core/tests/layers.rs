use pic_core::{
    ErrorCode, codec,
    composite::{BlendMode, Transform},
    document::{Raster, TargetId},
    limits::ResourceLimits,
    operation::{OperationSpec, geometry::Interpolation},
    pipeline::{Pipeline, PipelineSpec},
    result::Diagnostics,
};
use serde_json::{Value, json};
use std::path::Path;

fn spec(op: &str, target: &str, params: Value) -> OperationSpec {
    OperationSpec {
        op: op.into(),
        op_version: 1,
        target: TargetId(target.into()),
        params,
    }
}
fn run(base: &Path, raster: Raster, ops: Vec<OperationSpec>) -> pic_core::pipeline::Execution {
    Pipeline::new(
        PipelineSpec {
            schema_version: 1,
            operations: ops,
        },
        base,
        &ResourceLimits::default(),
    )
    .unwrap()
    .execute(raster)
    .unwrap()
}
fn raster(w: u32, h: u32, pixels: Vec<[f32; 4]>) -> Raster {
    Raster::from_linear_rgba(w, h, pixels, &ResourceLimits::default()).unwrap()
}
fn blank(w: u32, h: u32) -> Raster {
    raster(w, h, vec![[0.0; 4]; (w * h) as usize])
}
fn add(id: &str, source: &str) -> OperationSpec {
    spec(
        "layer_add",
        "canvas",
        json!({"id":id,"source":source,"name":"Same name"}),
    )
}
fn close(actual: [f32; 4], expected: [f32; 4]) {
    for (a, b) in actual.into_iter().zip(expected) {
        assert!((a - b).abs() < 2e-6, "{actual:?} != {expected:?}");
    }
}
fn save(base: &Path, name: &str, w: u32, h: u32, bytes: Vec<u8>) {
    image::RgbaImage::from_raw(w, h, bytes)
        .unwrap()
        .save(base.join(name))
        .unwrap();
}

#[test]
fn all_blend_modes_match_analytic_overlap_opacity_and_alpha() {
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), "layer.png", 1, 1, vec![255, 255, 255, 255]);
    let cases = [
        (BlendMode::Normal, [0.44, 0.52, 0.58, 0.625]),
        (BlendMode::Multiply, [0.312, 0.488, 0.57, 0.625]),
        (BlendMode::Screen, [0.448, 0.592, 0.7, 0.625]),
        (BlendMode::Overlay, [0.344, 0.544, 0.67, 0.625]),
    ];
    let mut doc = run(
        dir.path(),
        raster(1, 1, vec![[0.2, 0.6, 0.8, 0.5]]),
        vec![add("front", "layer.png")],
    )
    .document;
    doc.layers[1].raster = raster(1, 1, vec![[0.8, 0.4, 0.25, 0.5]]);
    doc.layers[1].opacity = 0.5;
    for (mode, expected) in cases {
        doc.layers[1].blend = mode;
        close(
            doc.render(&TargetId::canvas(), &ResourceLimits::default())
                .unwrap()
                .pixels()[0],
            expected,
        );
        close(
            pic_core::composite::blend([0.0, 0.0, 0.0, 0.0], [0.8, 0.4, 0.25, 0.5], 0.5, mode),
            [0.8, 0.4, 0.25, 0.25],
        );
        close(
            pic_core::composite::blend(
                [0.2, 0.6, 0.8, 0.5],
                [999.0, -999.0, 999.0, 0.0],
                1.0,
                mode,
            ),
            [0.2, 0.6, 0.8, 0.5],
        );
    }
    doc.layers[1].opacity = 0.0;
    close(
        doc.render(&TargetId::canvas(), &ResourceLimits::default())
            .unwrap()
            .pixels()[0],
        [0.2, 0.6, 0.8, 0.5],
    );
}

#[test]
fn stable_ids_drive_order_visibility_rename_removal_and_target_validation() {
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), "red.png", 1, 1, vec![255, 0, 0, 255]);
    save(dir.path(), "blue.png", 1, 1, vec![0, 0, 255, 255]);
    let ops = vec![add("red", "red.png"), add("blue", "blue.png")];
    close(
        run(dir.path(), blank(1, 1), ops.clone()).raster.pixels()[0],
        [0.0, 0.0, 1.0, 1.0],
    );
    let mut reordered = ops.clone();
    reordered.push(spec("layer_reorder", "blue", json!({"before":"red"})));
    reordered.push(spec("layer_set", "red", json!({"name":"blue"})));
    close(
        run(dir.path(), blank(1, 1), reordered.clone())
            .raster
            .pixels()[0],
        [1.0, 0.0, 0.0, 1.0],
    );
    reordered.push(spec("layer_set", "red", json!({"visible":false})));
    close(
        run(dir.path(), blank(1, 1), reordered.clone())
            .raster
            .pixels()[0],
        [0.0, 0.0, 1.0, 1.0],
    );
    reordered.push(spec("layer_remove", "red", json!({})));
    let removed = run(dir.path(), blank(1, 1), reordered.clone());
    assert_eq!(
        removed
            .document
            .layers
            .iter()
            .map(|l| l.id.0.as_str())
            .collect::<Vec<_>>(),
        ["base", "blue"]
    );
    for bad in [
        add("red", "red.png"),
        spec("invert", "missing", json!({})),
        spec("layer_reorder", "blue", json!({"before":"red"})),
        spec("layer_reorder", "blue", json!({"before":"blue"})),
    ] {
        let mut trial = reordered.clone();
        trial.push(bad);
        let error = Pipeline::new(
            PipelineSpec {
                schema_version: 1,
                operations: trial,
            },
            dir.path(),
            &ResourceLimits::default(),
        )
        .unwrap()
        .execute(blank(1, 1))
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidTarget);
    }
}

#[test]
fn encoded_masks_and_premultiplied_transform_taps_do_not_leak_masked_or_transparent_colors() {
    let dir = tempfile::tempdir().unwrap();
    save(
        dir.path(),
        "redblue.png",
        2,
        1,
        vec![255, 0, 0, 255, 0, 0, 255, 255],
    );
    save(
        dir.path(),
        "mask.png",
        2,
        1,
        vec![255, 255, 255, 255, 0, 0, 0, 255],
    );
    let mut transform = Transform {
        x: 0.5,
        ..Transform::default()
    };
    let mut ops = vec![
        add("front", "redblue.png"),
        spec("mask_set", "front", json!({"source":"mask.png"})),
        spec("layer_transform", "front", json!(transform)),
    ];
    let output = run(dir.path(), blank(3, 1), ops.clone());
    for p in &output.raster.pixels()[..2] {
        close(*p, [1.0, 0.0, 0.0, 0.5]);
    }
    close(output.raster.pixels()[2], [0.0; 4]);
    let mut rotated_ops = ops.clone();
    rotated_ops[2].params = json!(Transform {
        x: 1.0,
        y: 0.5,
        degrees: 37.0,
        ..Transform::default()
    });
    let rotated = run(dir.path(), blank(5, 5), rotated_ops);
    assert!(rotated.raster.pixels().iter().any(|p| p[3] > 0.0));
    for p in rotated.raster.pixels().iter().filter(|p| p[3] > 0.0) {
        close(*p, [1.0, 0.0, 0.0, p[3]]);
    }
    // A gray byte is encoded coverage 128/255, never the sRGB decoded ~0.216.
    save(
        dir.path(),
        "mask.png",
        2,
        1,
        vec![128, 128, 128, 255, 128, 128, 128, 128],
    );
    transform.x = 0.0;
    ops[2].params = json!(transform);
    let output = run(dir.path(), blank(3, 1), ops);
    close(output.raster.pixels()[0], [1.0, 0.0, 0.0, 128.0 / 255.0]);
    close(
        output.raster.pixels()[1],
        [0.0, 0.0, 1.0, (128.0 / 255.0) * (128.0 / 255.0)],
    );
    let mask = output
        .document
        .render(&TargetId("mask:front".into()), &ResourceLimits::default())
        .unwrap();
    let encoding = pic_core::codec::EncodeOptions::default()
        .resolve(&dir.path().join("preview.png"))
        .unwrap();
    let png = codec::encode(&mask, &encoding, &ResourceLimits::default()).unwrap();
    let decoded = image::load_from_memory(&png).unwrap().into_rgba8();
    assert_eq!(decoded.get_pixel(0, 0).0, [128, 128, 128, 255]);
    // Transparent blue has the same non-bleeding behavior without a mask.
    save(
        dir.path(),
        "hidden.png",
        2,
        1,
        vec![255, 0, 0, 255, 0, 0, 255, 0],
    );
    let output = run(
        dir.path(),
        blank(3, 1),
        vec![
            add("front", "hidden.png"),
            spec(
                "layer_transform",
                "front",
                json!(Transform {
                    x: 0.5,
                    ..Transform::default()
                }),
            ),
        ],
    );
    close(output.raster.pixels()[1], [1.0, 0.0, 0.0, 0.5]);
}

#[test]
fn transform_rotation_flip_scale_and_canvas_crop_preserve_local_pixels() {
    let dir = tempfile::tempdir().unwrap();
    save(
        dir.path(),
        "source.png",
        2,
        1,
        vec![255, 0, 0, 255, 0, 255, 0, 255],
    );
    let transform = Transform {
        x: 3.0,
        y: 1.0,
        scale_x: 2.0,
        scale_y: 1.0,
        degrees: 90.0,
        flip_x: true,
        filter: Interpolation::Nearest,
        ..Transform::default()
    };
    let output = run(
        dir.path(),
        blank(5, 6),
        vec![
            add("front", "source.png"),
            spec("layer_transform", "front", json!(transform)),
        ],
    );
    let layer = output.document.layer(&TargetId("front".into())).unwrap();
    assert_eq!((layer.raster.width(), layer.raster.height()), (2, 1));
    assert_eq!(layer.mapping().map([0.5, 0.5]), [2.5, 4.0]);
    for point in [[0.0, 0.0], [0.5, 0.5], [2.0, 1.0], [-10.0, 23.0]] {
        let roundtrip = layer.mapping().inverse().map(layer.mapping().map(point));
        assert_eq!(roundtrip, point);
    }
    for y in 1..5 {
        close(
            output.raster.pixels()[(y * 5 + 2) as usize],
            if y < 3 {
                [0.0, 1.0, 0.0, 1.0]
            } else {
                [1.0, 0.0, 0.0, 1.0]
            },
        );
    }
    let crop = run(
        dir.path(),
        blank(5, 6),
        vec![
            add("front", "source.png"),
            spec("layer_transform", "front", json!(transform)),
            spec("crop", "canvas", json!({"x":2,"y":1,"width":1,"height":4})),
        ],
    );
    assert_eq!((crop.raster.width(), crop.raster.height()), (1, 4));
    assert_eq!(
        crop.document.layers[1].raster.pixels(),
        layer.raster.pixels()
    );
    assert_eq!(
        crop.document.layers[1].mapping().map([0.5, 0.5]),
        [0.5, 3.0]
    );
    assert_eq!(
        crop.raster.pixels(),
        &[
            [0.0, 1.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0]
        ]
    );
}

#[test]
fn canvas_and_layer_space_selection_unions_map_through_real_transforms() {
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), "white.png", 3, 1, vec![255; 12]);
    let transform = Transform {
        x: 3.0,
        y: 1.0,
        degrees: 90.0,
        filter: Interpolation::Nearest,
        ..Transform::default()
    };
    let prefix = vec![
        add("subject", "white.png"),
        spec("layer_transform", "subject", json!(transform)),
    ];
    for (space, regions) in [
        (
            "canvas",
            json!([{"x":2,"y":1,"width":1,"height":1},{"x":2,"y":3,"width":1,"height":1}]),
        ),
        (
            "subject",
            json!([{"x":0,"y":0,"width":1,"height":1},{"x":2,"y":0,"width":1,"height":1}]),
        ),
    ] {
        let mut ops = prefix.clone();
        ops.push(spec(
            "selection_set",
            "canvas",
            json!({"space":space,"regions":regions}),
        ));
        ops.push(spec("invert", "subject", json!({})));
        let doc = run(dir.path(), blank(5, 5), ops).document;
        assert_eq!(
            doc.layers[1].raster.pixels(),
            &[[0.0, 0.0, 0.0, 1.0], [1.0; 4], [0.0, 0.0, 0.0, 1.0]]
        );
        assert_eq!(doc.selection.as_ref().unwrap().space.0, space);
    }
}

#[test]
fn invalid_masks_dimensions_transforms_and_layered_canvas_edits_are_structured_errors() {
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), "source.png", 2, 1, vec![255; 8]);
    save(dir.path(), "wrong-size.png", 1, 1, vec![255; 4]);
    save(
        dir.path(),
        "color.png",
        2,
        1,
        vec![255, 0, 0, 255, 255, 0, 0, 255],
    );
    let cases = [
        (
            spec("mask_set", "front", json!({"source":"wrong-size.png"})),
            ErrorCode::InvalidMask,
        ),
        (
            spec("mask_set", "front", json!({"source":"color.png"})),
            ErrorCode::InvalidMask,
        ),
        (
            spec("resize", "canvas", json!({"width":4,"filter":"nearest"})),
            ErrorCode::InvalidTarget,
        ),
        (
            spec(
                "layer_transform",
                "front",
                json!(Transform {
                    scale_x: 0.0,
                    ..Transform::default()
                }),
            ),
            ErrorCode::InvalidArgument,
        ),
        (
            spec(
                "selection_set",
                "canvas",
                json!({"space":"front","regions":[{"x":1,"y":0,"width":2,"height":1}]}),
            ),
            ErrorCode::InvalidArgument,
        ),
    ];
    for (bad, expected) in cases {
        let result = Pipeline::new(
            PipelineSpec {
                schema_version: 1,
                operations: vec![add("front", "source.png"), bad],
            },
            dir.path(),
            &ResourceLimits::default(),
        )
        .and_then(|p| p.execute(blank(2, 2)));
        assert_eq!(result.unwrap_err().code, expected);
    }
    let input = codec::load(
        &dir.path().join("source.png"),
        &ResourceLimits::default(),
        &mut Diagnostics::default(),
    )
    .unwrap();
    assert_eq!(input.raster.width(), 2);
}
