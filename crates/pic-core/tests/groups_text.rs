use pic_core::{
    ErrorCode,
    composite::Transform,
    document::{Document, LayerKind, Raster, TargetId},
    limits::ResourceLimits,
    operation::{
        OperationSpec,
        geometry::{Interpolation, RgbaColor},
    },
    pipeline::{Execution, Pipeline, PipelineSpec},
    text::{self, TextAlign, TextParams},
};
use serde_json::{Value, json};
use std::{fs, path::Path};

const FONT: &[u8] = include_bytes!("../../../tests/fonts/DejaVuSans.ttf");
fn op(name: &str, target: &str, params: Value) -> OperationSpec {
    OperationSpec {
        op: name.into(),
        op_version: 1,
        target: TargetId(target.into()),
        params,
    }
}
fn group(id: &str, w: u32, h: u32) -> OperationSpec {
    op(
        "group_add",
        "canvas",
        json!({"id":id,"name":id,"width":w,"height":h}),
    )
}
fn parent(id: &str, parent: &str) -> OperationSpec {
    op("layer_parent", id, json!({"parent":parent,"before":null}))
}
fn add(id: &str, source: &str) -> OperationSpec {
    op(
        "layer_add",
        "canvas",
        json!({"id":id,"name":id,"source":source}),
    )
}
fn adjustment(id: &str, w: u32, h: u32, name: &str, params: Value) -> OperationSpec {
    op(
        "adjustment_add",
        "canvas",
        json!({"id":id,"name":id,"width":w,"height":h,"adjustment":{"op":name,"params":params}}),
    )
}
fn raster(w: u32, h: u32, p: [f32; 4]) -> Raster {
    Raster::from_linear_rgba(w, h, vec![p; (w * h) as usize], &ResourceLimits::default()).unwrap()
}
fn pipeline(dir: &Path, ops: Vec<OperationSpec>, limits: &ResourceLimits) -> Pipeline {
    Pipeline::new(
        PipelineSpec {
            schema_version: 1,
            operations: ops,
        },
        dir,
        limits,
    )
    .unwrap()
}
fn run(dir: &Path, input: Raster, ops: Vec<OperationSpec>) -> Execution {
    pipeline(dir, ops, &ResourceLimits::default())
        .execute(input)
        .unwrap()
}
fn save(dir: &Path, name: &str, w: u32, h: u32, rgba: [u8; 4]) {
    image::RgbaImage::from_pixel(w, h, image::Rgba(rgba))
        .save(dir.join(name))
        .unwrap();
}
fn close(a: [f32; 4], b: [f32; 4]) {
    for (x, y) in a.into_iter().zip(b) {
        assert!((x - y).abs() < 2e-6, "{a:?} != {b:?}");
    }
}
fn transform(x: f64, y: f64) -> Value {
    json!(Transform {
        x,
        y,
        filter: Interpolation::Nearest,
        ..Transform::default()
    })
}

#[test]
fn isolated_groups_apply_bounds_order_visibility_transform_and_opacity_once() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    save(dir, "red.png", 2, 1, [255, 0, 0, 255]);
    save(dir, "blue.png", 2, 1, [0, 0, 255, 255]);
    let mut ops = vec![
        group("g", 2, 1),
        add("red", "red.png"),
        parent("red", "g"),
        add("blue", "blue.png"),
        parent("blue", "g"),
        op("layer_transform", "g", transform(1.0, 1.0)),
        op("layer_set", "g", json!({"opacity":0.5})),
    ];
    let image = run(dir, raster(4, 3, [0.0, 1.0, 0.0, 1.0]), ops.clone());
    close(image.raster.pixels()[5], [0.0, 0.5, 0.5, 1.0]);
    close(image.raster.pixels()[4], [0.0, 1.0, 0.0, 1.0]);
    close(image.raster.pixels()[7], [0.0, 1.0, 0.0, 1.0]);
    // Moving a group carries the complete subtree at its one sibling stack position.
    let mut moved = ops.clone();
    moved.push(op("layer_reorder", "g", json!({"before":"base"})));
    close(
        run(dir, raster(4, 3, [0.0, 1.0, 0.0, 1.0]), moved)
            .raster
            .pixels()[5],
        [0.0, 1.0, 0.0, 1.0],
    );
    ops.push(op("layer_reorder", "blue", json!({"before":"red"})));
    close(
        run(dir, raster(4, 3, [0.0, 1.0, 0.0, 1.0]), ops.clone())
            .raster
            .pixels()[5],
        [0.5, 0.5, 0.0, 1.0],
    );
    ops.push(op("layer_set", "g", json!({"visible":false})));
    let hidden = run(dir, raster(4, 3, [0.0, 1.0, 0.0, 1.0]), ops);
    close(hidden.raster.pixels()[5], [0.0, 1.0, 0.0, 1.0]);
    assert!(
        hidden
            .document
            .render(&TargetId("red".into()), &ResourceLimits::default())
            .unwrap()
            .pixels()
            .iter()
            .all(|p| p[3] == 0.0)
    );
}

#[test]
fn nested_coordinates_masks_selection_and_canvas_crop_keep_local_editing_state() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    save(dir, "red.png", 2, 1, [255, 0, 0, 255]);
    save(dir, "mask.png", 3, 2, [128, 128, 128, 255]);
    let ops = vec![
        group("outer", 5, 4),
        group("inner", 3, 2),
        parent("inner", "outer"),
        op("layer_transform", "outer", transform(2.0, 1.0)),
        op("layer_transform", "inner", transform(1.0, 1.0)),
        op("mask_set", "inner", json!({"source":"mask.png"})),
        add("red", "red.png"),
        parent("red", "inner"),
    ];
    let original = run(dir, raster(8, 6, [0.0; 4]), ops.clone());
    let matrix = original
        .document
        .world_mapping(&TargetId("red".into()))
        .unwrap();
    assert_eq!(matrix.map([0.5, 0.5]), [3.5, 2.5]);
    assert_eq!(matrix.inverse().map(matrix.map([1.25, -2.5])), [1.25, -2.5]);
    close(original.raster.pixels()[19], [1.0, 0.0, 0.0, 128.0 / 255.0]);
    let mut edited = ops.clone();
    edited.extend([
        op(
            "selection_set",
            "canvas",
            json!({"space":"canvas","regions":[{"x":3,"y":2,"width":1,"height":1}]}),
        ),
        op("invert", "red", json!({})),
        op("crop", "canvas", json!({"x":2,"y":1,"width":4,"height":4})),
    ]);
    let result = run(dir, raster(8, 6, [0.0; 4]), edited);
    assert_eq!(
        result
            .document
            .layer(&TargetId("red".into()))
            .unwrap()
            .raster
            .pixels(),
        &[[0.0, 1.0, 1.0, 1.0], [1.0, 0.0, 0.0, 1.0]]
    );
    close(result.raster.pixels()[5], [0.0, 1.0, 1.0, 128.0 / 255.0]);
    assert_eq!(
        result
            .document
            .world_mapping(&TargetId("red".into()))
            .unwrap()
            .map([0.5, 0.5]),
        [1.5, 1.5]
    );
    let mut rotated = ops;
    rotated.push(op(
        "layer_transform",
        "outer",
        json!(Transform {
            x: 5.0,
            y: 0.0,
            scale_x: 2.0,
            scale_y: 1.0,
            degrees: 90.0,
            flip_x: true,
            filter: Interpolation::Nearest,
            ..Transform::default()
        }),
    ));
    let rotated = run(dir, raster(8, 12, [0.0; 4]), rotated);
    let matrix = rotated
        .document
        .world_mapping(&TargetId("red".into()))
        .unwrap();
    for point in [[0.5, 0.5], [1.25, 0.75], [-2.0, 9.0]] {
        assert_eq!(matrix.inverse().map(matrix.map(point)), point);
    }
    assert!(rotated.raster.pixels().iter().any(|p| p[3] > 0.0));
}

#[test]
fn clipping_uses_transformed_base_alpha_mask_opacity_visibility_and_chains() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    save(dir, "red.png", 1, 1, [255, 0, 0, 128]);
    save(dir, "blue.png", 3, 1, [0, 0, 255, 255]);
    save(dir, "green.png", 3, 1, [0, 255, 0, 255]);
    save(dir, "mask.png", 1, 1, [128, 128, 128, 255]);
    let mut ops = vec![
        add("red", "red.png"),
        op("mask_set", "red", json!({"source":"mask.png"})),
        op("layer_transform", "red", transform(1.0, 0.0)),
        op("layer_set", "red", json!({"opacity":0.5})),
        add("blue", "blue.png"),
        op("layer_clip", "blue", json!({"base":"red"})),
        op("layer_set", "blue", json!({"opacity":0.5})),
        add("green", "green.png"),
        op("layer_clip", "green", json!({"base":"blue"})),
    ];
    let result = run(dir, raster(3, 1, [1.0; 4]), ops.clone());
    let isolated = result
        .document
        .render(&TargetId("green".into()), &ResourceLimits::default())
        .unwrap();
    close(isolated.pixels()[0], [0.0; 4]);
    close(isolated.pixels()[2], [0.0; 4]);
    close(
        isolated.pixels()[1],
        [0.0, 1.0, 0.0, (128.0 / 255.0_f32).powi(2) * 0.25],
    );
    ops.push(op("layer_set", "red", json!({"visible":false})));
    let hidden = run(dir, raster(3, 1, [1.0; 4]), ops);
    assert!(
        hidden
            .document
            .render(&TargetId("green".into()), &ResourceLimits::default())
            .unwrap()
            .pixels()
            .iter()
            .all(|p| p[3] == 0.0)
    );
    assert_eq!(hidden.raster.pixels(), &[[1.0; 4]; 3]);
}

#[test]
fn groups_can_supply_and_receive_clipping_without_leaking_scope_alpha() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    save(dir, "red.png", 1, 1, [255, 0, 0, 128]);
    save(dir, "blue.png", 3, 1, [0, 0, 255, 255]);
    let output = run(
        dir,
        raster(3, 1, [0.0; 4]),
        vec![
            group("base_group", 3, 1),
            add("red", "red.png"),
            parent("red", "base_group"),
            group("front_group", 3, 1),
            add("blue", "blue.png"),
            parent("blue", "front_group"),
            op("layer_clip", "front_group", json!({"base":"base_group"})),
            op("layer_set", "front_group", json!({"opacity":0.5})),
        ],
    );
    let isolated = output
        .document
        .render(&TargetId("front_group".into()), &ResourceLimits::default())
        .unwrap();
    close(isolated.pixels()[0], [0.0, 0.0, 1.0, 64.0 / 255.0]);
    close(isolated.pixels()[1], [0.0; 4]);
}

#[test]
fn adjustments_change_only_lower_scope_rgb_and_preserve_alpha_and_source_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    save(dir, "red.png", 1, 1, [255, 0, 0, 128]);
    save(dir, "blue.png", 1, 1, [0, 0, 255, 255]);
    save(dir, "mask.png", 2, 1, [128, 128, 128, 255]);
    let mut ops = vec![
        group("g", 2, 1),
        op("layer_transform", "g", transform(1.0, 0.0)),
        add("red", "red.png"),
        parent("red", "g"),
        adjustment("inv", 2, 1, "invert", json!({})),
        parent("inv", "g"),
        op("layer_set", "inv", json!({"opacity":0.5})),
        op("mask_set", "inv", json!({"source":"mask.png"})),
    ];
    let input = raster(4, 1, [0.1, 0.2, 0.3, 0.4]);
    let output = run(dir, input.clone(), ops.clone());
    let adjusted = output
        .document
        .render(&TargetId("inv".into()), &ResourceLimits::default())
        .unwrap();
    close(
        adjusted.pixels()[1],
        [
            1.0 - 64.0 / 255.0,
            64.0 / 255.0,
            64.0 / 255.0,
            128.0 / 255.0,
        ],
    );
    close(output.raster.pixels()[0], [0.1, 0.2, 0.3, 0.4]);
    close(output.raster.pixels()[2], [0.1, 0.2, 0.3, 0.4]);
    assert_eq!(
        output
            .document
            .layer(&TargetId("red".into()))
            .unwrap()
            .raster
            .pixels()[0],
        [1.0, 0.0, 0.0, 128.0 / 255.0]
    );
    ops.extend([add("blue", "blue.png"), parent("blue", "g")]);
    close(
        run(dir, input.clone(), ops.clone()).raster.pixels()[1],
        [0.0, 0.0, 1.0, 1.0],
    );
    ops.push(adjustment("root", 4, 1, "invert", json!({})));
    let root = run(dir, input, ops);
    close(root.raster.pixels()[0], [0.9, 0.8, 0.7, 0.4]);
    close(root.raster.pixels()[1], [1.0, 1.0, 0.0, 1.0]);
}

#[test]
fn adjustment_modes_match_existing_point_operations_and_clip_weight_is_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    let cases = [
        (
            "adjust",
            json!({"exposure":1.0,"brightness":-0.1,"contrast":1.3,"saturation":0.8}),
        ),
        (
            "levels",
            json!({"channel":"red","input_black":0.1,"input_white":0.8,"gamma":1.2,"output_black":0.0,"output_white":1.0}),
        ),
        (
            "curves",
            json!({"channel":"rgb","points":[[0,0],[0.5,0.3],[1,1]]}),
        ),
        ("grayscale", json!({})),
        ("invert", json!({})),
    ];
    for (name, params) in cases {
        let input = raster(2, 1, [0.2, 0.4, 1.5, 0.25]);
        let direct = run(dir, input.clone(), vec![op(name, "canvas", params.clone())]);
        let output = run(dir, input, vec![adjustment("a", 2, 1, name, params)]);
        assert_eq!(direct.raster.pixels(), output.raster.pixels());
    }
    let result = run(
        dir,
        raster(1, 1, [0.2, 0.4, 0.6, 0.5]),
        vec![
            adjustment("a", 1, 1, "invert", json!({})),
            op("layer_clip", "a", json!({"base":"base"})),
        ],
    );
    close(result.raster.pixels()[0], [0.5, 0.5, 0.5, 0.5]);
}

#[test]
fn invalid_hierarchy_references_cycles_orders_removals_and_generated_pixel_edits_fail() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    let prefix = vec![
        group("g", 1, 1),
        group("nested", 1, 1),
        parent("nested", "g"),
        group("other", 1, 1),
    ];
    for (bad, code) in [
        (parent("base", "missing"), ErrorCode::InvalidTarget),
        (parent("g", "base"), ErrorCode::InvalidHierarchy),
        (parent("g", "nested"), ErrorCode::DependencyCycle),
        (parent("g", "g"), ErrorCode::DependencyCycle),
        (
            op("layer_clip", "g", json!({"base":"g"})),
            ErrorCode::DependencyCycle,
        ),
        (
            op("layer_clip", "g", json!({"base":"other"})),
            ErrorCode::InvalidHierarchy,
        ),
        (
            op("layer_clip", "other", json!({"base":"nested"})),
            ErrorCode::InvalidHierarchy,
        ),
        (
            op("layer_clip", "other", json!({"base":"missing"})),
            ErrorCode::InvalidTarget,
        ),
        (
            op("layer_reorder", "other", json!({"before":"nested"})),
            ErrorCode::InvalidHierarchy,
        ),
        (
            op("layer_remove", "g", json!({})),
            ErrorCode::InvalidHierarchy,
        ),
        (op("invert", "g", json!({})), ErrorCode::InvalidTarget),
    ] {
        let mut ops = prefix.clone();
        ops.push(bad);
        assert_eq!(
            pipeline(dir, ops, &ResourceLimits::default())
                .execute(raster(1, 1, [0.0; 4]))
                .unwrap_err()
                .code,
            code
        );
    }
    let mut document = run(dir, raster(1, 1, [0.0; 4]), prefix).document;
    document.layers[0].clip = Some(TargetId("other".into()));
    let idx = document
        .layers
        .iter()
        .position(|l| l.id.0 == "other")
        .unwrap();
    document.layers[idx].clip = Some(TargetId("base".into()));
    assert_eq!(
        document
            .render(&TargetId::canvas(), &ResourceLimits::default())
            .unwrap_err()
            .code,
        ErrorCode::DependencyCycle
    );
}

#[test]
fn flat_two_layer_budget_counts_sources_once_and_scope_scratch_is_still_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    save(dir, "front.png", 8, 4, [255, 0, 0, 128]);
    // 2 source rasters (32*32 bytes) + three scope surfaces (32*48) = 2560.
    // The old blanket *4+64 formula incorrectly charged 6144 for this exact scene.
    let limits = ResourceLimits {
        max_buffer_bytes: 2560,
        ..ResourceLimits::default()
    };
    let pipeline = pipeline(dir, vec![add("front", "front.png")], &limits);
    let result = pipeline.execute(raster(8, 4, [0.0; 4])).unwrap();
    close(result.raster.pixels()[0], [1.0, 0.0, 0.0, 128.0 / 255.0]);
    assert_eq!(
        result
            .document
            .render(
                &TargetId::canvas(),
                &ResourceLimits {
                    max_buffer_bytes: 2559,
                    ..limits.clone()
                }
            )
            .unwrap_err()
            .code,
        ErrorCode::ResourceLimit
    );
    // A retained clipping alpha plane requires another 32*4 bytes.
    let mut clipped = result.document.clone();
    clipped.layers[1].clip = Some(TargetId("base".into()));
    assert_eq!(
        clipped.validate(&limits).unwrap_err().code,
        ErrorCode::ResourceLimit
    );
    clipped
        .validate(&ResourceLimits {
            max_buffer_bytes: 2688,
            ..limits
        })
        .unwrap();
    // Group recursion and adjustment interpolation remain separately charged.
    let grouped = run(
        dir,
        raster(8, 4, [0.0; 4]),
        vec![
            group("g", 8, 4),
            add("front", "front.png"),
            parent("front", "g"),
            adjustment("a", 8, 4, "invert", json!({})),
            parent("a", "g"),
        ],
    )
    .document;
    // Four retained rasters=2048; root back=512 + group adjustment scratch=2048.
    grouped
        .validate(&ResourceLimits {
            max_buffer_bytes: 4608,
            ..ResourceLimits::default()
        })
        .unwrap();
    assert_eq!(
        grouped
            .validate(&ResourceLimits {
                max_buffer_bytes: 4607,
                ..ResourceLimits::default()
            })
            .unwrap_err()
            .code,
        ErrorCode::ResourceLimit
    );
}

#[test]
fn legacy_fast_path_rejects_generated_kinds_parents_and_clips() {
    let mut doc = Document::from_raster(raster(1, 1, [1.0; 4]));
    doc.layers[0].kind = LayerKind::Group;
    assert!(
        doc.render(&TargetId::canvas(), &ResourceLimits::default())
            .is_err()
    );
    doc.layers[0].kind = LayerKind::Raster;
    doc.layers[0].parent = Some(TargetId("base".into()));
    assert!(
        doc.render(&TargetId::canvas(), &ResourceLimits::default())
            .is_err()
    );
    doc.layers[0].parent = None;
    doc.layers[0].clip = Some(TargetId("base".into()));
    assert!(
        doc.render(&TargetId::canvas(), &ResourceLimits::default())
            .is_err()
    );
}

fn text_params(content: &str) -> TextParams {
    TextParams {
        text: content.into(),
        font: "explicit.ttf".into(),
        size: 24.0,
        line_height: 32.0,
        width: 200,
        height: 120,
        align: TextAlign::Left,
        color: RgbaColor([255, 128, 0, 128]),
    }
}
#[test]
fn true_shaping_has_ligatures_kerning_combining_marks_and_unicode_pixels() {
    let ligature = text::layout(&text_params("ffi"), FONT).unwrap();
    assert_eq!(
        ligature.glyphs.len(),
        1,
        "the font's ffi ligature must be used"
    );
    let kern = text::layout(&text_params("AV"), FONT).unwrap().line_widths[0];
    let separate = text::layout(&text_params("A"), FONT).unwrap().line_widths[0]
        + text::layout(&text_params("V"), FONT).unwrap().line_widths[0];
    assert!(kern < separate);
    let combined =
        text::render(&text_params("e\u{301}"), FONT, &ResourceLimits::default()).unwrap();
    let precomposed = text::render(&text_params("é"), FONT, &ResourceLimits::default()).unwrap();
    assert_eq!(combined.pixels(), precomposed.pixels());
    let unicode = text::render(
        &text_params("Café ffi\nΩμέγα\nПривет"),
        FONT,
        &ResourceLimits::default(),
    )
    .unwrap();
    for (start, end) in [(0, 32), (32, 64), (64, 96)] {
        assert!(
            unicode.pixels()[start * 200..end * 200]
                .iter()
                .any(|p| p[3] > 0.0)
        );
    }
    assert!(
        unicode
            .pixels()
            .iter()
            .any(|p| p[3] > 0.0 && p[3] < 128.0 / 255.0)
    );
    assert!(
        unicode
            .pixels()
            .iter()
            .all(|p| p[3] <= 128.0 / 255.0 + 1e-6)
    );
}

#[test]
fn line_breaks_alignment_and_clipping_are_replayable_layout_and_pixels() {
    let left = text::layout(&text_params("AV\nAV"), FONT).unwrap();
    assert_eq!(left.glyphs[2].baseline - left.glyphs[0].baseline, 32.0);
    let mut center = text_params("AV\nAV");
    center.align = TextAlign::Center;
    let centered = text::layout(&center, FONT).unwrap();
    assert!((centered.glyphs[0].x - (200.0 - left.line_widths[0]) / 2.0).abs() < 1e-5);
    center.align = TextAlign::Right;
    let right = text::layout(&center, FONT).unwrap();
    assert!((right.glyphs[0].x - (200.0 - left.line_widths[0])).abs() < 1e-5);
    // A 20px wider centered box shifts glyphs exactly 10px; compare actual coverage.
    center.align = TextAlign::Center;
    center.width = 220;
    let wider = text::render(&center, FONT, &ResourceLimits::default()).unwrap();
    center.width = 200;
    let normal = text::render(&center, FONT, &ResourceLimits::default()).unwrap();
    for y in 0..120 {
        for x in 0..200 {
            close(
                wider.pixels()[y * 220 + x + 10],
                normal.pixels()[y * 200 + x],
            );
        }
    }
    let mut clipped = text_params("long long long");
    clipped.width = 8;
    clipped.height = 8;
    let image = text::render(&clipped, FONT, &ResourceLimits::default()).unwrap();
    assert_eq!(image.pixels().len(), 64);
}

#[test]
fn missing_fonts_glyphs_and_unsupported_text_never_fall_back() {
    assert_eq!(
        text::render(&text_params("A"), b"not a font", &ResourceLimits::default())
            .unwrap_err()
            .code,
        ErrorCode::InvalidFont
    );
    let error = text::render(&text_params("中文"), FONT, &ResourceLimits::default()).unwrap_err();
    assert_eq!(error.code, ErrorCode::MissingGlyph);
    assert!(error.message.contains("U+4E2D"));
    for content in ["A\tB", "A\r\nB", "A\u{202e}B", "العربية", "AΩ"] {
        assert_eq!(
            text::render(&text_params(content), FONT, &ResourceLimits::default())
                .unwrap_err()
                .code,
            ErrorCode::UnsupportedText,
            "{content}"
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let params = text_params("Café");
    let ops = vec![op(
        "text_add",
        "canvas",
        json!({"id":"t","name":"text","text":params}),
    )];
    let error = pipeline(dir.path(), ops.clone(), &ResourceLimits::default())
        .execute(raster(200, 120, [0.0; 4]))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::FileNotFound);
    fs::write(dir.path().join("explicit.ttf"), FONT).unwrap();
    let result = run(dir.path(), raster(200, 120, [0.0; 4]), ops);
    assert!(matches!(
        result.document.layers[1].kind,
        LayerKind::Text { .. }
    ));
    assert!(result.raster.pixels().iter().any(|p| p[3] > 0.0));
}
