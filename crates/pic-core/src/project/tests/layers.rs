use super::*;
use crate::{composite::Transform, document::Document};
use serde_json::{Value, json};

fn spec(op: &str, target: &str, params: Value) -> OperationSpec {
    OperationSpec {
        op: op.into(),
        op_version: 1,
        target: TargetId(target.into()),
        params,
    }
}
fn add(source: &str) -> OperationSpec {
    spec(
        "layer_add",
        "canvas",
        json!({"id":"subject","source":source,"name":"Subject"}),
    )
}
fn same_document(a: &Document, b: &Document) {
    assert_eq!(
        serde_json::to_value(a.info()).unwrap(),
        serde_json::to_value(b.info()).unwrap()
    );
    for (a, b) in a.layers.iter().zip(&b.layers) {
        super::replay::exact(&a.raster, &b.raster);
        if let (Some(a), Some(b)) = (&a.mask, &b.mask) {
            super::replay::exact(a, b);
        }
    }
}
fn mask_file(dir: &Path) {
    image::RgbaImage::from_fn(8, 4, |x, _| image::Rgba([128, 128, 128, (x * 31) as u8]))
        .save(dir.join("mask.png"))
        .unwrap();
}

#[test]
fn project_binding_preserves_tighter_pipeline_admission_and_asset_limits() {
    let (dir, input, path) = fixture();
    image::RgbaImage::from_pixel(8, 6, image::Rgba([42, 17, 93, 255]))
        .save(&input)
        .unwrap();
    create(&input, &path);
    let original = fs::read(path.join("manifest.json")).unwrap();
    for tight in [
        ResourceLimits {
            max_dimension: 4,
            ..ResourceLimits::default()
        },
        ResourceLimits {
            max_pixels: 32,
            ..ResourceLimits::default()
        },
        ResourceLimits {
            max_buffer_bytes: 512,
            ..ResourceLimits::default()
        },
    ] {
        let pipeline = Pipeline::single(OperationSpec::identity(), dir.path(), &tight).unwrap();
        assert_eq!(
            apply(&path, &pipeline, "r0").unwrap_err().code,
            ErrorCode::ResourceLimit
        );
        assert_eq!(fs::read(path.join("manifest.json")).unwrap(), original);
    }
    let tiny = dir.path().join("tiny.png");
    image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255]))
        .save(&tiny)
        .unwrap();
    let small = dir.path().join("small.pic");
    create(&tiny, &small);
    let before = fs::read(small.join("manifest.json")).unwrap();
    let bytes = fs::metadata(&input).unwrap().len();
    for tight in [
        ResourceLimits {
            max_dimension: 4,
            ..ResourceLimits::default()
        },
        ResourceLimits {
            max_pixels: 4,
            ..ResourceLimits::default()
        },
        ResourceLimits {
            max_buffer_bytes: 512,
            ..ResourceLimits::default()
        },
        ResourceLimits {
            max_input_bytes: bytes - 1,
            ..ResourceLimits::default()
        },
    ] {
        let pipeline = Pipeline::single(add("input.png"), dir.path(), &tight).unwrap();
        assert_eq!(
            apply(&small, &pipeline, "r0").unwrap_err().code,
            ErrorCode::ResourceLimit
        );
        assert_eq!(fs::read(small.join("manifest.json")).unwrap(), before);
    }
    // Same encoded source supplied through an existing asset reference remains bounded too.
    let hash = Storage::open(&small)
        .unwrap()
        .put_asset(&fs::read(&input).unwrap())
        .unwrap();
    let tight = ResourceLimits {
        max_input_bytes: bytes - 1,
        ..ResourceLimits::default()
    };
    let pipeline = Pipeline::single(add(&format!("asset:{hash}")), dir.path(), &tight).unwrap();
    assert_eq!(
        apply(&small, &pipeline, "r0").unwrap_err().code,
        ErrorCode::ResourceLimit
    );
    assert_eq!(fs::read(small.join("manifest.json")).unwrap(), before);
    // Mask imports also observe the caller's encoded-byte limit.
    apply(
        &path,
        &super::pipeline(dir.path(), vec![add("input.png")]),
        "r0",
    )
    .unwrap();
    image::GrayImage::from_pixel(8, 6, image::Luma([128]))
        .save(dir.path().join("large-mask.png"))
        .unwrap();
    let tight = ResourceLimits {
        max_input_bytes: fs::metadata(dir.path().join("large-mask.png"))
            .unwrap()
            .len()
            - 1,
        ..ResourceLimits::default()
    };
    let pipeline = Pipeline::single(
        spec("mask_set", "subject", json!({"source":"large-mask.png"})),
        dir.path(),
        &tight,
    )
    .unwrap();
    let before = fs::read(path.join("manifest.json")).unwrap();
    assert_eq!(
        apply(&path, &pipeline, "r1").unwrap_err().code,
        ErrorCode::ResourceLimit
    );
    assert_eq!(fs::read(path.join("manifest.json")).unwrap(), before);
}

#[test]
fn layered_checkpoints_preserve_all_float_bits_metadata_masks_and_future_edits() {
    let (dir, input, path) = fixture();
    mask_file(dir.path());
    create(&input, &path);
    let transform = Transform {
        x: 2.125,
        y: -0.75,
        scale_x: 0.75,
        scale_y: 1.25,
        degrees: 37.0,
        flip_y: true,
        ..Transform::default()
    };
    let ops = vec![
        add("input.png"),
        spec(
            "adjust",
            "subject",
            json!({"exposure":4.123456789012345,"brightness":-0.23,"contrast":1.31,"saturation":1.0}),
        ),
        spec("mask_set", "subject", json!({"source":"mask.png"})),
        spec("layer_transform", "subject", json!(transform)),
        spec(
            "selection_set",
            "canvas",
            json!({"space":"subject","regions":[{"x":1,"y":1,"width":3,"height":2}]}),
        ),
    ];
    apply(&path, &pipeline(dir.path(), ops), "r0").unwrap();
    let project = open(&path);
    let direct = project.restore(None, &mut Diagnostics::default()).unwrap();
    assert!(
        direct.document.layers[1]
            .raster
            .pixels()
            .iter()
            .any(|p| p[0] < 0.0)
    );
    assert!(
        direct.document.layers[1]
            .raster
            .pixels()
            .iter()
            .any(|p| p[0] > 1.0)
    );
    let checkpoint = project
        .checkpoint(None, &mut Diagnostics::default())
        .unwrap();
    assert!(checkpoint.disk_stored);
    let bytes = fs::read(
        path.join("checkpoints")
            .join(format!("{}.bin", checkpoint.key)),
    )
    .unwrap();
    assert_eq!(&bytes[..8], b"PICDOC02");
    let restored = open(&path)
        .restore(None, &mut Diagnostics::default())
        .unwrap();
    assert_eq!(restored.replay.reused_steps, 5);
    same_document(&direct.document, &restored.document);
    super::replay::exact(&direct.raster, &restored.raster);
    let change = apply(
        &path,
        &pipeline(
            dir.path(),
            vec![
                spec("invert", "subject", json!({})),
                spec("layer_set", "base", json!({"visible":false})),
            ],
        ),
        "r5",
    )
    .unwrap();
    assert_eq!(change.replay.reused_steps, 5);
    let resumed = open(&path)
        .restore(None, &mut Diagnostics::default())
        .unwrap();
    open(&path)
        .clear_cache(&mut Diagnostics::default())
        .unwrap();
    let replayed = open(&path)
        .restore(None, &mut Diagnostics::default())
        .unwrap();
    assert_eq!(replayed.operations_replayed, 7);
    same_document(&resumed.document, &replayed.document);
    super::replay::exact(&resumed.raster, &replayed.raster);
    // Arbitrary f32 bit patterns, including hidden HDR and signed zeros, survive extended codec.
    let mut document = direct.document;
    document.layers[1].raster = Raster::from_linear_rgba(
        8,
        4,
        vec![[-0.0, f32::from_bits(1), -f32::MAX, 0.0]; 32],
        &ResourceLimits::default(),
    )
    .unwrap();
    let cached = cache::CachedRaster {
        raster: direct.raster,
        canvas: ImageSize {
            width: 8,
            height: 4,
        },
        document: Some(document.clone()),
        layer_to_canvas: None,
    };
    let key = storage::hash(b"layer bit precision");
    assert!(
        project
            .cache_put(
                storage::DerivedKind::Checkpoint,
                &key,
                cached,
                &mut Diagnostics::default()
            )
            .0
    );
    let decoded = open(&path)
        .cache_get(
            storage::DerivedKind::Checkpoint,
            &key,
            &mut Diagnostics::default(),
        )
        .unwrap()
        .0;
    same_document(&document, &decoded.document.unwrap());
}

#[test]
fn layered_checkpoints_restore_with_default_and_maximum_history_limits() {
    let (dir, input, path) = fixture();
    create(&input, &path);
    apply(
        &path,
        &pipeline(
            dir.path(),
            vec![
                add("input.png"),
                spec(
                    "layer_add",
                    "canvas",
                    json!({"id":"removed","source":"input.png","name":"Removed"}),
                ),
                spec("layer_remove", "removed", json!({})),
            ],
        ),
        "r0",
    )
    .unwrap();
    let project = open(&path);
    let original = project.restore(None, &mut Diagnostics::default()).unwrap();
    assert_eq!(original.document.layers.len(), 2);
    assert_eq!(original.document.used_layer_ids.len(), 3);
    let checkpoint = project
        .checkpoint(None, &mut Diagnostics::default())
        .unwrap();
    assert!(checkpoint.disk_stored);
    for max_history_operations in [ResourceLimits::default().max_history_operations, usize::MAX] {
        let limits = ResourceLimits {
            max_history_operations,
            ..ResourceLimits::default()
        };
        let restored = Project::open(&path, &limits, &mut Diagnostics::default())
            .unwrap()
            .restore(None, &mut Diagnostics::default())
            .unwrap();
        let hit = restored.replay.cache_hit.unwrap();
        assert_eq!(hit.tier, "disk");
        assert_eq!(hit.key, checkpoint.key);
        assert_eq!(restored.replay.reused_steps, 3);
        assert_eq!(restored.operations_replayed, 0);
        same_document(&original.document, &restored.document);
        super::replay::exact(&original.raster, &restored.raster);
    }
}

#[test]
fn checkpoint_preview_hits_verify_layer_and_mask_assets_and_corruption_replays() {
    let (dir, input, path) = fixture();
    mask_file(dir.path());
    create(&input, &path);
    apply(
        &path,
        &pipeline(
            dir.path(),
            vec![
                add("input.png"),
                spec("mask_set", "subject", json!({"source":"mask.png"})),
            ],
        ),
        "r0",
    )
    .unwrap();
    let project = open(&path);
    let cp = project
        .checkpoint(None, &mut Diagnostics::default())
        .unwrap();
    let request = || PreviewRequest {
        export: ExportRequest {
            revision: None,
            output: &input,
            encoding: EncodeOptions::default(),
            overwrite: true,
        },
        target: TargetId("mask:subject".into()),
        region: None,
        width: Some(4),
        height: None,
        filter: Interpolation::Bilinear,
    };
    project
        .preview(request(), &mut Diagnostics::default())
        .unwrap();
    let before = fs::read(&input).unwrap();
    let mask = &project.commits()[0].steps[1].input_assets[1];
    let asset = path.join("assets").join(&mask.sha256);
    let bytes = fs::read(&asset).unwrap();
    for missing in [false, true] {
        if missing {
            fs::remove_file(&asset).unwrap();
        } else {
            fs::write(&asset, b"corrupt").unwrap();
        }
        let expected = if missing {
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
            expected
        );
        assert_eq!(
            project
                .preview(request(), &mut Diagnostics::default())
                .unwrap_err()
                .code,
            expected
        );
        assert_eq!(fs::read(&input).unwrap(), before);
        fs::write(&asset, &bytes).unwrap();
    }
    let file = path.join("checkpoints").join(format!("{}.bin", cp.key));
    let valid = fs::read(&file).unwrap();
    let exact = project.restore(None, &mut Diagnostics::default()).unwrap();
    for corrupt in [&b"bad"[..], &valid[..valid.len() - 5]] {
        fs::write(&file, corrupt).unwrap();
        let restored = open(&path)
            .restore(None, &mut Diagnostics::default())
            .unwrap();
        assert_eq!(restored.operations_replayed, 2);
        same_document(&exact.document, &restored.document);
    }
}

#[test]
fn layered_asset_and_publication_failures_keep_manifest_and_stale_writers_do_not_import() {
    let (dir, input, path) = fixture();
    mask_file(dir.path());
    create(&input, &path);
    let ops = pipeline(dir.path(), vec![add("mask.png")]);
    let before = fs::read(path.join("manifest.json")).unwrap();
    for stage in [
        "asset_write",
        "ops_write",
        "manifest_write",
        "manifest_publish",
    ] {
        assert_eq!(
            storage::with_failure(stage, || apply(&path, &ops, "r0"))
                .unwrap_err()
                .code,
            ErrorCode::IoError
        );
        assert_eq!(fs::read(path.join("manifest.json")).unwrap(), before);
        assert_eq!(
            open(&path)
                .restore(None, &mut Diagnostics::default())
                .unwrap()
                .document
                .layers
                .len(),
            1
        );
    }
    apply(&path, &ops, "r0").unwrap();
    fs::remove_file(dir.path().join("mask.png")).unwrap();
    assert_eq!(
        apply(&path, &ops, "r0").unwrap_err().code,
        ErrorCode::RevisionConflict
    );
}

#[test]
fn templates_reject_layer_pixel_targets_but_allow_legacy_prefixes() {
    let (dir, input, path) = fixture();
    create(&input, &path);
    apply(
        &path,
        &pipeline(dir.path(), vec![spec("invert", "base", json!({}))]),
        "r0",
    )
    .unwrap();
    assert_eq!(
        open(&path).operation_template(None).unwrap_err().code,
        ErrorCode::UnsupportedTemplate
    );
    let mut old = OperationSpec::identity();
    old.target = TargetId::canvas();
    let project = open(&path);
    assert!(project.operation_template(Some("r0")).is_ok());
    assert!(old.validate().unwrap().template_safe());
}

#[test]
fn layered_restore_rejects_flat_checkpoint_even_with_matching_identity_and_checksum() {
    let (dir, input, path) = fixture();
    create(&input, &path);
    apply(&path, &pipeline(dir.path(), vec![add("input.png")]), "r0").unwrap();
    let project = open(&path);
    let original = project.restore(None, &mut Diagnostics::default()).unwrap();
    let cp = project
        .checkpoint(None, &mut Diagnostics::default())
        .unwrap();
    project.cache_put(
        storage::DerivedKind::Checkpoint,
        &cp.key,
        cache::CachedRaster {
            raster: original.raster,
            canvas: ImageSize {
                width: 8,
                height: 4,
            },
            document: None,
            layer_to_canvas: None,
        },
        &mut Diagnostics::default(),
    );
    for project in [project, open(&path)] {
        let restored = project.restore(None, &mut Diagnostics::default()).unwrap();
        assert!(restored.replay.cache_hit.is_none());
        assert_eq!(restored.operations_replayed, 1);
        same_document(&original.document, &restored.document);
    }
}
