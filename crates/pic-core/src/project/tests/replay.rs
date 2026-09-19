use super::*;
use crate::operation::geometry::CropParams;

fn adjustment(exposure: f64, brightness: f64) -> OperationSpec {
    Operation::Adjust(AdjustParams {
        exposure,
        brightness,
        contrast: 1.0,
        saturation: 1.0,
    })
    .to_spec()
}

pub(super) fn exact(a: &Raster, b: &Raster) {
    assert_eq!((a.width(), a.height()), (b.width(), b.height()));
    assert_eq!(a.pixels().len(), b.pixels().len());
    for (a, b) in a.pixels().iter().zip(b.pixels()) {
        assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits));
    }
}

fn restore(project: &Project, revision: &str) -> RestoredRevision {
    project
        .restore(Some(revision), &mut Diagnostics::default())
        .unwrap()
}

fn checkpoint(project: &Project, revision: &str) -> CheckpointResult {
    project
        .checkpoint(Some(revision), &mut Diagnostics::default())
        .unwrap()
}

fn authoritative_files(path: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = vec![(
        path.join("manifest.json"),
        fs::read(path.join("manifest.json")).unwrap(),
    )];
    for folder in ["assets", "ops"] {
        files.extend(fs::read_dir(path.join(folder)).unwrap().map(|entry| {
            let path = entry.unwrap().path();
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        }));
    }
    files.sort();
    files
}

fn preview(
    project: &Project,
    revision: &str,
    output: &Path,
    region: Option<CropParams>,
    width: Option<u32>,
    filter: Interpolation,
) -> ProjectPreview {
    project
        .preview(
            PreviewRequest {
                export: ExportRequest {
                    revision: Some(revision),
                    output,
                    encoding: EncodeOptions::default(),
                    overwrite: true,
                },
                target: TargetId::canvas(),
                region,
                width,
                height: None,
                filter,
            },
            &mut Diagnostics::default(),
        )
        .unwrap()
}

#[test]
fn checkpoints_match_direct_float_state_and_corruption_falls_back_to_earlier_prefix() {
    let (dir, input, path) = fixture();
    create(&input, &path);
    let ops = vec![
        adjustment(3.123456789012345, -0.123456789012345),
        Operation::Curves(CurvesParams {
            channel: Channel::Red,
            points: vec![[0.0, 0.02], [0.4, 0.7], [1.0, 0.98]],
        })
        .to_spec(),
        Operation::Resize(ResizeParams {
            width: Some(11),
            height: None,
            filter: Interpolation::Bilinear,
        })
        .to_spec(),
        adjustment(-3.123456789012345, 0.0),
    ];
    let pipeline = pipeline(dir.path(), ops);
    apply(&path, &pipeline, "r0").unwrap();
    let source = codec::load(
        &input,
        &ResourceLimits::default(),
        &mut Diagnostics::default(),
    )
    .unwrap()
    .raster;
    let direct = pipeline.execute(source).unwrap().raster;
    let project = open(&path);
    let authority = authoritative_files(&path);
    let high = restore(&project, "r1").raster;
    assert!(high.pixels().iter().any(|p| p[0] < 0.0));
    assert!(high.pixels().iter().any(|p| p[0] > 1.0));
    checkpoint(&project, "r1");
    let cp = checkpoint(&project, "r2");
    assert!(cp.disk_stored);
    let resumed = restore(&open(&path), "r4");
    assert_eq!(resumed.replay.reused_steps, 2);
    assert_eq!(resumed.operations_replayed, 2);
    assert_eq!(resumed.replay.cache_hit.unwrap().tier, "disk");
    exact(&resumed.raster, &direct);
    let file = path.join("checkpoints").join(format!("{}.bin", cp.key));
    let original = fs::read(&file).unwrap();
    for case in 0..5 {
        let mut bytes = original.clone();
        match case {
            0 => bytes.clear(),
            1 => bytes.truncate(100),
            2 => bytes[95] ^= 0x80,
            3 => bytes[7] = b'9', // unknown snapshot version
            4 => bytes[8] ^= 1,   // wrong identity
            _ => unreachable!(),
        }
        fs::write(&file, bytes).unwrap();
        let resumed = restore(&open(&path), "r4");
        assert_eq!(resumed.replay.cache_hit.unwrap().revision.0, "r1");
        assert_eq!(resumed.operations_replayed, 3);
        exact(&resumed.raster, &direct);
    }
    fs::remove_dir_all(path.join("checkpoints")).unwrap();
    fs::remove_dir_all(path.join("cache")).unwrap();
    let replayed = restore(&open(&path), "r4");
    assert_eq!(replayed.operations_replayed, 4);
    assert!(replayed.replay.cache_hit.is_none());
    exact(&replayed.raster, &direct);
    assert_eq!(authoritative_files(&path), authority);
}

#[test]
fn twenty_step_revise_reuses_prefix_and_never_changes_old_revisions() {
    let (dir, input, path) = fixture();
    create(&input, &path);
    let mut ops = vec![adjustment(0.05, -0.002); 20];
    apply(&path, &pipeline(dir.path(), ops.clone()), "r0").unwrap();
    let original_files = authoritative_files(&path);
    let original = restore(&open(&path), "r20").raster;
    for revision in ["r2", "r17", "r20"] {
        checkpoint(&open(&path), revision);
    }
    let changed = Project::revise(
        ReviseRequest {
            project: &path,
            step_revision: "r18",
            params: adjustment(0.7, 0.01).params,
            expected_revision: "r20",
        },
        &ResourceLimits::default(),
        &mut Diagnostics::default(),
    )
    .unwrap();
    assert_eq!(changed.revision.0, "r23");
    assert_eq!(changed.replay.reused_steps, 17);
    assert_eq!(changed.replay.cache_hit.unwrap().revision.0, "r17");
    assert_eq!(
        changed.replay.recomputed_revisions,
        (21..=23)
            .map(|i| RevisionId(format!("r{i}")))
            .collect::<Vec<_>>()
    );
    ops[17] = adjustment(0.7, 0.01);
    let source = codec::load(
        &input,
        &ResourceLimits::default(),
        &mut Diagnostics::default(),
    )
    .unwrap()
    .raster;
    let first_direct = pipeline(dir.path(), ops.clone())
        .execute(source.clone())
        .unwrap()
        .raster;
    exact(&restore(&open(&path), "r23").raster, &first_direct);
    checkpoint(&open(&path), "r23"); // later caches must not leak across the changed third step
    let changed = Project::revise(
        ReviseRequest {
            project: &path,
            step_revision: "r3",
            params: adjustment(-0.2, 0.005).params,
            expected_revision: "r23",
        },
        &ResourceLimits::default(),
        &mut Diagnostics::default(),
    )
    .unwrap();
    assert_eq!(changed.revision.0, "r41");
    assert_eq!(changed.replay.reused_steps, 2);
    assert_eq!(changed.replay.cache_hit.unwrap().revision.0, "r2");
    assert_eq!(
        changed.replay.recomputed_revisions,
        (24..=41)
            .map(|i| RevisionId(format!("r{i}")))
            .collect::<Vec<_>>()
    );
    ops[2] = adjustment(-0.2, 0.005);
    let direct = pipeline(dir.path(), ops).execute(source).unwrap().raster;
    exact(&restore(&open(&path), "r41").raster, &direct);
    exact(&restore(&open(&path), "r20").raster, &original);
    exact(&restore(&open(&path), "r23").raster, &first_direct);
    for (file, bytes) in original_files {
        if file.file_name().unwrap() != "manifest.json" {
            assert_eq!(fs::read(file).unwrap(), bytes);
        }
    }
    let before = authoritative_files(&path);
    for (step_revision, params, expected, code) in [
        (
            "r3",
            adjustment(1.0, 0.0).params,
            "r23",
            ErrorCode::RevisionConflict,
        ),
        (
            "r18",
            adjustment(1.0, 0.0).params,
            "r41",
            ErrorCode::RevisionNotFound,
        ),
        (
            "r24",
            serde_json::json!({"exposure":1}),
            "r41",
            ErrorCode::InvalidArgument,
        ),
    ] {
        assert_eq!(
            Project::revise(
                ReviseRequest {
                    project: &path,
                    step_revision,
                    params,
                    expected_revision: expected
                },
                &ResourceLimits::default(),
                &mut Diagnostics::default()
            )
            .unwrap_err()
            .code,
            code
        );
        assert_eq!(authoritative_files(&path), before);
    }
}

#[test]
fn cache_budgets_bound_both_tiers_and_never_evict_required_assets() {
    let (dir, input, path) = fixture();
    create(&input, &path);
    apply(
        &path,
        &pipeline(dir.path(), vec![adjustment(0.1, 0.0); 3]),
        "r0",
    )
    .unwrap();
    let authority = authoritative_files(&path);
    let limits = ResourceLimits {
        max_cache_disk_bytes: 632,
        max_cache_memory_bytes: 1500,
        ..ResourceLimits::default()
    };
    let project = Project::open(&path, &limits, &mut Diagnostics::default()).unwrap();
    for revision in ["r1", "r2", "r3"] {
        assert!(checkpoint(&project, revision).disk_stored);
        let bytes: u64 = ["cache", "checkpoints"]
            .iter()
            .flat_map(|folder| fs::read_dir(path.join(folder)).unwrap())
            .map(|entry| entry.unwrap().metadata().unwrap().len())
            .sum();
        assert!(bytes <= limits.max_cache_disk_bytes);
    }
    assert_eq!(
        restore(&project, "r3").replay.cache_hit.unwrap().tier,
        "memory"
    );
    assert_eq!(restore(&open(&path), "r1").operations_replayed, 1);
    preview(
        &project,
        "r3",
        &dir.path().join("small.png"),
        None,
        Some(2),
        Interpolation::Bilinear,
    );
    let bytes: u64 = ["cache", "checkpoints"]
        .iter()
        .flat_map(|folder| fs::read_dir(path.join(folder)).unwrap())
        .map(|entry| entry.unwrap().metadata().unwrap().len())
        .sum();
    assert!(bytes <= 632); // preview and checkpoint share one budget

    // Memory-only cache: two 512-byte rasters fit. Creating r3 touches r2 as its prefix.
    let limits = ResourceLimits {
        max_cache_disk_bytes: 0,
        max_cache_memory_bytes: 1024,
        ..limits
    };
    let project = Project::open(&path, &limits, &mut Diagnostics::default()).unwrap();
    for revision in ["r1", "r2"] {
        let cp = checkpoint(&project, revision);
        assert!(!cp.disk_stored);
        assert!(cp.memory_stored);
    }
    assert_eq!(
        restore(&project, "r1").replay.cache_hit.unwrap().tier,
        "memory"
    );
    checkpoint(&project, "r3");
    let evicted = restore(&project, "r1");
    assert!(evicted.replay.cache_hit.is_none());
    assert_eq!(evicted.operations_replayed, 1);
    assert_eq!(
        restore(&project, "r2").replay.cache_hit.unwrap().tier,
        "memory"
    );
    let zero = ResourceLimits {
        max_cache_memory_bytes: 0,
        ..limits
    };
    let project = Project::open(&path, &zero, &mut Diagnostics::default()).unwrap();
    let cp = checkpoint(&project, "r3");
    assert!(!cp.disk_stored && !cp.memory_stored);
    assert_eq!(restore(&project, "r3").operations_replayed, 3);
    assert_eq!(authoritative_files(&path), authority);
}

#[test]
fn clear_removes_zero_length_and_truncated_managed_files_only() {
    let (dir, input, path) = fixture();
    create(&input, &path);
    apply(
        &path,
        &pipeline(dir.path(), vec![OperationSpec::identity()]),
        "r0",
    )
    .unwrap();
    let authority = authoritative_files(&path);
    let project = open(&path);
    for folder in ["checkpoints", "cache"] {
        fs::create_dir(path.join(folder)).unwrap();
    }
    for include_nonempty in [false, true] {
        for folder in ["checkpoints", "cache"] {
            fs::write(
                path.join(folder).join(format!("{}.bin", "0".repeat(64))),
                [],
            )
            .unwrap();
            if include_nonempty {
                fs::write(
                    path.join(folder).join(format!("{}.bin", "1".repeat(64))),
                    b"truncated",
                )
                .unwrap();
                // A later zero-byte entry used to survive once total became zero.
                fs::write(
                    path.join(folder).join(format!("{}.bin", "2".repeat(64))),
                    [],
                )
                .unwrap();
            }
            fs::write(path.join(folder).join("unmanaged.txt"), b"keep").unwrap();
        }
        project.clear_cache(&mut Diagnostics::default()).unwrap();
        for folder in ["checkpoints", "cache"] {
            let entries: Vec<_> = fs::read_dir(path.join(folder))
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            assert_eq!(entries, ["unmanaged.txt"]);
        }
        assert_eq!(authoritative_files(&path), authority);
        assert_eq!(restore(&project, "r1").operations_replayed, 1);
    }
}

#[test]
fn preview_region_mappings_cache_variants_and_full_export_stay_independent() {
    let (dir, input, path) = fixture();
    create(&input, &path);
    apply(
        &path,
        &pipeline(
            dir.path(),
            vec![
                adjustment(0.5, 0.0),
                Operation::Resize(ResizeParams {
                    width: Some(12),
                    height: Some(8),
                    filter: Interpolation::Bilinear,
                })
                .to_spec(),
            ],
        ),
        "r0",
    )
    .unwrap();
    let before = authoritative_files(&path);
    let region = CropParams {
        x: 1,
        y: 1,
        width: 6,
        height: 3,
    };
    let output = dir.path().join("preview.png");
    let project = open(&path);
    let first = preview(
        &project,
        "r1",
        &output,
        Some(region.clone()),
        Some(3),
        Interpolation::Bilinear,
    );
    assert_eq!(
        first.canvas,
        ImageSize {
            width: 8,
            height: 4
        }
    );
    assert_eq!(
        first.preview_size,
        ImageSize {
            width: 3,
            height: 2
        }
    );
    for point in [[0.0, 0.0], [0.5, 0.5], [1.3, 0.75], [3.0, 2.0]] {
        let canvas = first.coordinates.preview_to_canvas.map(point);
        let mapped = first.coordinates.canvas_to_preview.map(canvas);
        for i in 0..2 {
            assert!((point[i] - mapped[i]).abs() < 1e-12);
        }
    }
    assert_eq!(
        first.coordinates.preview_to_canvas.map([0.0, 0.0]),
        [1.0, 1.0]
    );
    assert_eq!(
        first.coordinates.preview_to_canvas.map([3.0, 2.0]),
        [7.0, 4.0]
    );
    let hit = preview(
        &open(&path),
        "r1",
        &output,
        Some(region.clone()),
        Some(3),
        Interpolation::Bilinear,
    );
    assert_eq!(hit.rendered.replay.cache_hit.unwrap().kind, "preview");
    assert_eq!(hit.rendered.operations_replayed, 0);
    for (revision, region, width, filter) in [
        ("r2", Some(region.clone()), Some(3), Interpolation::Bilinear),
        ("r1", None, Some(3), Interpolation::Bilinear),
        ("r1", Some(region.clone()), Some(4), Interpolation::Bilinear),
        ("r1", Some(region.clone()), Some(3), Interpolation::Nearest),
    ] {
        let result = preview(&open(&path), revision, &output, region, width, filter);
        assert!(result.rendered.replay.cache_hit.is_none());
    }
    let exported = project
        .export(
            ExportRequest {
                revision: None,
                output: &dir.path().join("full.png"),
                encoding: EncodeOptions::default(),
                overwrite: false,
            },
            &mut Diagnostics::default(),
        )
        .unwrap();
    assert_eq!((exported.width, exported.height), (12, 8));
    assert_eq!(exported.operations_replayed, 2);
    assert_eq!(authoritative_files(&path), before);
    let stale = Project::apply(
        ApplyRequest {
            project: &path,
            pipeline: &pipeline(dir.path(), vec![OperationSpec::identity()]),
            expected_revision: "r1",
            revision: Some("r1"),
        },
        &ResourceLimits::default(),
        &mut Diagnostics::default(),
    )
    .unwrap_err();
    assert_eq!(stale.code, ErrorCode::RevisionConflict);
}

#[test]
fn cached_pixels_never_hide_missing_changed_assets_or_changed_content_identity() {
    let (dir, input, path) = fixture();
    create(&input, &path);
    apply(
        &path,
        &pipeline(dir.path(), vec![adjustment(0.5, 0.0)]),
        "r0",
    )
    .unwrap();
    let project = open(&path);
    let cp = checkpoint(&project, "r1");
    preview(
        &project,
        "r1",
        &dir.path().join("p.png"),
        None,
        Some(2),
        Interpolation::Bilinear,
    );
    let asset = path.join("assets").join(&project.manifest.source.sha256);
    let bytes = fs::read(&asset).unwrap();
    for missing in [false, true] {
        if missing {
            fs::remove_file(&asset).unwrap();
        } else {
            let mut changed = bytes.clone();
            changed[20] ^= 1;
            fs::write(&asset, changed).unwrap();
        }
        assert_eq!(
            project
                .restore(None, &mut Diagnostics::default())
                .err()
                .unwrap()
                .code,
            if missing {
                ErrorCode::AssetMissing
            } else {
                ErrorCode::IntegrityMismatch
            }
        );
        let error = project
            .preview(
                PreviewRequest {
                    export: ExportRequest {
                        revision: Some("r1"),
                        output: &dir.path().join("p.png"),
                        encoding: EncodeOptions::default(),
                        overwrite: true,
                    },
                    target: TargetId::canvas(),
                    region: None,
                    width: Some(2),
                    height: None,
                    filter: Interpolation::Bilinear,
                },
                &mut Diagnostics::default(),
            )
            .unwrap_err();
        assert_eq!(
            error.code,
            if missing {
                ErrorCode::AssetMissing
            } else {
                ErrorCode::IntegrityMismatch
            }
        );
        fs::write(&asset, &bytes).unwrap();
    }
    // Same operations, different embedded source: even copying all old snapshots cannot hit.
    let second_input = dir.path().join("different.png");
    image::RgbaImage::from_pixel(8, 4, image::Rgba([0, 30, 200, 123]))
        .save(&second_input)
        .unwrap();
    let second_path = dir.path().join("second.pic");
    create(&second_input, &second_path);
    apply(
        &second_path,
        &pipeline(dir.path(), vec![adjustment(0.5, 0.0)]),
        "r0",
    )
    .unwrap();
    fs::create_dir(second_path.join("checkpoints")).unwrap();
    fs::copy(
        path.join("checkpoints").join(format!("{}.bin", cp.key)),
        second_path
            .join("checkpoints")
            .join(format!("{}.bin", cp.key)),
    )
    .unwrap();
    let restored = restore(&open(&second_path), "r1");
    assert!(restored.replay.cache_hit.is_none());
    assert_eq!(restored.operations_replayed, 1);
}

#[test]
fn unsafe_cache_paths_and_failed_snapshot_writes_fall_back_without_touching_authority() {
    use std::os::unix::fs::symlink;
    let (dir, input, path) = fixture();
    create(&input, &path);
    apply(
        &path,
        &pipeline(dir.path(), vec![adjustment(0.1, 0.0)]),
        "r0",
    )
    .unwrap();
    let authority = authoritative_files(&path);
    let project = open(&path);
    let cp = storage::with_failure("cache_write", || checkpoint(&project, "r1"));
    assert!(!cp.disk_stored);
    assert!(cp.memory_stored);
    assert_eq!(fs::read_dir(path.join("checkpoints")).unwrap().count(), 0);
    assert_eq!(restore(&open(&path), "r1").operations_replayed, 1);
    fs::remove_dir(path.join("checkpoints")).unwrap();
    symlink(path.join("assets"), path.join("checkpoints")).unwrap();
    let cp = checkpoint(&open(&path), "r1");
    assert!(!cp.disk_stored);
    assert_eq!(restore(&open(&path), "r1").operations_replayed, 1);
    assert_eq!(
        project
            .clear_cache(&mut Diagnostics::default())
            .unwrap_err()
            .code,
        ErrorCode::UnsafePath
    );
    assert_eq!(authoritative_files(&path), authority);
}
