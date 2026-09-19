use super::*;
use crate::{composite::Transform, document::LayerKind};
use serde_json::{Value, json};
use sha2::Digest;

const FONT: &[u8] = include_bytes!("../../../../../tests/fonts/DejaVuSans.ttf");
fn op(name: &str, target: &str, params: Value) -> OperationSpec {
    OperationSpec {
        op: name.into(),
        op_version: 1,
        target: TargetId(target.into()),
        params,
    }
}
fn group(id: &str, width: u32, height: u32) -> OperationSpec {
    op(
        "group_add",
        "canvas",
        json!({"id":id,"name":id,"width":width,"height":height}),
    )
}
fn parent(id: &str, parent: &str) -> OperationSpec {
    op("layer_parent", id, json!({"parent":parent,"before":null}))
}
fn text(font: &str, content: &str) -> Value {
    json!({"font":font,"text":content,"size":14,"line_height":18,"width":64,"height":48,"align":"center","color":[255,255,255,255]})
}
fn same(a: &RestoredRevision, b: &RestoredRevision) {
    assert_eq!(
        serde_json::to_value(a.document.info()).unwrap(),
        serde_json::to_value(b.document.info()).unwrap()
    );
    super::replay::exact(&a.raster, &b.raster);
    for (a, b) in a.document.layers.iter().zip(&b.document.layers) {
        super::replay::exact(&a.raster, &b.raster);
        if let (Some(a), Some(b)) = (&a.mask, &b.mask) {
            super::replay::exact(a, b);
        }
    }
}

#[test]
fn editable_generated_layers_survive_checkpoints_history_moving_fonts_and_cold_replay() {
    let (dir, input, path) = fixture();
    image::RgbaImage::from_pixel(72, 56, image::Rgba([12, 34, 56, 128]))
        .save(&input)
        .unwrap();
    fs::write(dir.path().join("font.ttf"), FONT).unwrap();
    image::RgbaImage::from_pixel(8, 4, image::Rgba([255, 0, 0, 128]))
        .save(dir.path().join("subject.png"))
        .unwrap();
    image::GrayImage::from_pixel(8, 4, image::Luma([128]))
        .save(dir.path().join("mask.png"))
        .unwrap();
    create(&input, &path);
    let ops = vec![
        group("g", 64, 48),
        op(
            "layer_transform",
            "g",
            json!(Transform {
                x: 3.125,
                y: 2.5,
                ..Transform::default()
            }),
        ),
        op(
            "layer_add",
            "canvas",
            json!({"id":"subject","source":"subject.png","name":"Subject"}),
        ),
        parent("subject", "g"),
        op("mask_set", "subject", json!({"source":"mask.png"})),
        op(
            "layer_add",
            "canvas",
            json!({"id":"clip","source":"subject.png","name":"Clip"}),
        ),
        parent("clip", "g"),
        op("layer_clip", "clip", json!({"base":"subject"})),
        op(
            "adjustment_add",
            "canvas",
            json!({"id":"adjust","name":"Adjustment","width":64,"height":48,"adjustment":{"op":"adjust","params":{"exposure":3.123456789,"brightness":-0.2,"contrast":1.3,"saturation":0.7}}}),
        ),
        parent("adjust", "g"),
        op(
            "text_add",
            "canvas",
            json!({"id":"caption","name":"Caption","text":text("font.ttf","Café ffi\nΩμέγα")}),
        ),
        parent("caption", "g"),
    ];
    let change = apply(&path, &pipeline(dir.path(), ops), "r0").unwrap();
    let first = change.revision.0;
    let project = open(&path);
    let before = project.restore(None, &mut Diagnostics::default()).unwrap();
    let cp = project
        .checkpoint(None, &mut Diagnostics::default())
        .unwrap();
    assert!(cp.disk_stored);
    let cached = open(&path)
        .restore(None, &mut Diagnostics::default())
        .unwrap();
    assert_eq!(cached.operations_replayed, 0);
    same(&before, &cached);
    let font = match &before
        .document
        .layer(&TargetId("caption".into()))
        .unwrap()
        .kind
    {
        LayerKind::Text { params } => params.font.clone(),
        _ => panic!("editable text required"),
    };
    assert!(font.starts_with("asset:"));
    // Original imports are not dependencies after binding; moving the whole project is safe.
    for file in ["input.png", "subject.png", "mask.png", "font.ttf"] {
        fs::remove_file(dir.path().join(file)).unwrap();
    }
    let moved = dir.path().join("moved.pic");
    fs::rename(&path, &moved).unwrap();
    let editing = vec![
        op("text_set", "caption", text(&font, "Office é\nПривет")),
        op(
            "adjustment_set",
            "adjust",
            json!({"op":"curves","params":{"channel":"red","points":[[0,0],[0.5,0.75],[1,1]]}}),
        ),
        op("layer_set", "g", json!({"opacity":0.75})),
    ];
    let change = apply(&moved, &pipeline(dir.path(), editing), &first).unwrap();
    assert_eq!(change.replay.reused_steps, 12); // 12 logical prefix steps
    let after = open(&moved)
        .restore(None, &mut Diagnostics::default())
        .unwrap();
    assert_ne!(before.raster.pixels(), after.raster.pixels());
    Project::undo(
        &moved,
        &change.revision.0,
        &ResourceLimits::default(),
        &mut Diagnostics::default(),
    )
    .unwrap();
    same(
        &before,
        &open(&moved)
            .restore(None, &mut Diagnostics::default())
            .unwrap(),
    );
    Project::redo(
        &moved,
        &first,
        &ResourceLimits::default(),
        &mut Diagnostics::default(),
    )
    .unwrap();
    same(
        &after,
        &open(&moved)
            .restore(None, &mut Diagnostics::default())
            .unwrap(),
    );
    open(&moved)
        .checkpoint(None, &mut Diagnostics::default())
        .unwrap();
    let snapshot = open(&moved)
        .restore(None, &mut Diagnostics::default())
        .unwrap();
    assert_eq!(snapshot.operations_replayed, 0);
    same(&after, &snapshot);
    open(&moved)
        .clear_cache(&mut Diagnostics::default())
        .unwrap();
    let replayed = open(&moved)
        .restore(None, &mut Diagnostics::default())
        .unwrap();
    assert_eq!(replayed.operations_replayed, 15);
    same(&after, &replayed);
    assert_eq!(
        open(&moved).operation_template(None).unwrap_err().code,
        ErrorCode::UnsupportedTemplate
    );
}

#[test]
fn font_dependencies_are_verified_even_when_text_pixels_and_previews_are_cached() {
    let (dir, input, path) = fixture();
    fs::write(dir.path().join("font.ttf"), FONT).unwrap();
    create(&input, &path);
    apply(
        &path,
        &pipeline(
            dir.path(),
            vec![op(
                "text_add",
                "canvas",
                json!({"id":"t","name":"Text","text":text("font.ttf","é")}),
            )],
        ),
        "r0",
    )
    .unwrap();
    let project = open(&path);
    project
        .checkpoint(None, &mut Diagnostics::default())
        .unwrap();
    let output = dir.path().join("preview.png");
    let request = || PreviewRequest {
        export: ExportRequest {
            revision: None,
            output: &output,
            encoding: EncodeOptions::default(),
            overwrite: true,
        },
        target: TargetId("t".into()),
        region: None,
        width: None,
        height: None,
        filter: Interpolation::Nearest,
    };
    project
        .preview(request(), &mut Diagnostics::default())
        .unwrap();
    let original = fs::read(&output).unwrap();
    let hash = &project.commits()[0].steps[0].input_assets[1].sha256;
    let file = path.join("assets").join(hash);
    for missing in [true, false] {
        if missing {
            fs::remove_file(&file).unwrap();
        } else {
            fs::write(&file, b"different font bytes").unwrap();
        }
        let code = if missing {
            ErrorCode::AssetMissing
        } else {
            ErrorCode::IntegrityMismatch
        };
        assert_eq!(
            project
                .restore(None, &mut Diagnostics::default())
                .err()
                .unwrap()
                .code,
            code
        );
        assert_eq!(
            project
                .preview(request(), &mut Diagnostics::default())
                .unwrap_err()
                .code,
            code
        );
        assert_eq!(fs::read(&output).unwrap(), original);
        fs::write(&file, FONT).unwrap();
    }
    // A snapshot from the prior renderer must not be interpreted with new metadata semantics.
    let checkpoint = project
        .checkpoint(None, &mut Diagnostics::default())
        .unwrap();
    let file = path
        .join("checkpoints")
        .join(format!("{}.bin", checkpoint.key));
    let mut bytes = fs::read(&file).unwrap();
    bytes[..8].copy_from_slice(b"PICDOC02");
    let len = bytes.len() - 32;
    let checksum = sha2::Sha256::digest(&bytes[..len]);
    bytes[len..].copy_from_slice(&checksum);
    fs::write(file, bytes).unwrap();
    assert_eq!(
        open(&path)
            .restore(None, &mut Diagnostics::default())
            .unwrap()
            .operations_replayed,
        1
    );
}

#[test]
fn twenty_seven_extreme_group_transforms_reject_unreliable_inverse_without_publication() {
    // Both matrix and inverse entries are finite, but inf-inf in their product is NaN.
    assert!(
        crate::composite::Affine([1e160, 0.0, 1e160, 1e-160, 0.0, 0.0])
            .checked_inverse()
            .is_none()
    );
    let (dir, input, path) = fixture();
    image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 0]))
        .save(&input)
        .unwrap();
    create(&input, &path);
    let mut groups = Vec::new();
    for i in 0..27 {
        groups.push(group(&format!("g{i}"), 1, 1));
        if i > 0 {
            groups.push(parent(&format!("g{i}"), &format!("g{}", i - 1)));
        }
    }
    let change = apply(&path, &pipeline(dir.path(), groups), "r0").unwrap();
    let manifest = fs::read(path.join("manifest.json")).unwrap();
    let transforms = (0..27)
        .map(|i| {
            op(
                "layer_transform",
                &format!("g{i}"),
                json!(Transform {
                    scale_x: 1e6,
                    scale_y: 1e6,
                    ..Transform::default()
                }),
            )
        })
        .collect();
    let error = apply(&path, &pipeline(dir.path(), transforms), &change.revision.0).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidHierarchy);
    assert!(error.message.contains("inverse"));
    assert_eq!(fs::read(path.join("manifest.json")).unwrap(), manifest);
    let normal = (0..27)
        .map(|i| {
            op(
                "layer_transform",
                &format!("g{i}"),
                json!(Transform {
                    x: 0.125,
                    y: 0.25,
                    scale_x: 1.01,
                    scale_y: 0.99,
                    degrees: 1.0,
                    ..Transform::default()
                }),
            )
        })
        .collect();
    apply(&path, &pipeline(dir.path(), normal), &change.revision.0).unwrap();
    let restored = open(&path)
        .restore(None, &mut Diagnostics::default())
        .unwrap();
    let mapping = restored
        .document
        .world_mapping(&TargetId("g26".into()))
        .unwrap();
    let point = [0.125, 0.875];
    let result = mapping.inverse().map(mapping.map(point));
    assert!((result[0] - point[0]).abs() < 1e-10 && (result[1] - point[1]).abs() < 1e-10);
}
