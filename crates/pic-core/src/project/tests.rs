use super::*;
use crate::operation::{
    Operation, OperationSpec,
    adjustments::{AdjustParams, Channel, CurvesParams},
    geometry::{Interpolation, ResizeParams},
};

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.png");
    image::RgbaImage::from_fn(8, 4, |x, y| {
        image::Rgba([(x * 31) as u8, (y * 63) as u8, 173, (x * 35) as u8])
    })
    .save(&input)
    .unwrap();
    let project = dir.path().join("work.pic");
    (dir, input, project)
}

fn create(input: &Path, project: &Path) {
    Project::create(
        input,
        project,
        &ResourceLimits::default(),
        &mut Diagnostics::default(),
    )
    .unwrap();
}

fn open(path: &Path) -> Project {
    Project::open(
        path,
        &ResourceLimits::default(),
        &mut Diagnostics::default(),
    )
    .unwrap()
}

fn pipeline(base: &Path, specs: Vec<OperationSpec>) -> Pipeline {
    Pipeline::new(
        PipelineSpec {
            schema_version: 1,
            operations: specs,
        },
        base,
        &ResourceLimits::default(),
    )
    .unwrap()
}

fn apply(project: &Path, pipeline: &Pipeline, expected: &str) -> Result<ProjectChange> {
    Project::apply(
        ApplyRequest {
            project,
            pipeline,
            expected_revision: expected,
            revision: None,
        },
        &ResourceLimits::default(),
        &mut Diagnostics::default(),
    )
}

#[test]
fn create_asset_manifest_and_directory_failures_publish_nothing() {
    let (dir, input, project) = fixture();
    for stage in [
        "asset_write",
        "manifest_write",
        "manifest_publish",
        "create_publish",
    ] {
        let error = storage::with_failure(stage, || {
            Project::create(
                &input,
                &project,
                &ResourceLimits::default(),
                &mut Diagnostics::default(),
            )
        })
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::IoError, "{stage}");
        assert!(!project.exists(), "{stage}");
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            1,
            "staging cleanup: {stage}"
        );
    }
    create(&input, &project);
    let old = fs::read(project.join("manifest.json")).unwrap();
    assert_eq!(
        Project::create(
            &input,
            &project,
            &ResourceLimits::default(),
            &mut Diagnostics::default()
        )
        .unwrap_err()
        .code,
        ErrorCode::OutputExists
    );
    assert_eq!(fs::read(project.join("manifest.json")).unwrap(), old);
}

#[test]
fn asset_import_deduplicates_and_partial_asset_writes_preserve_existing_project() {
    let (_dir, input, project) = fixture();
    create(&input, &project);
    let storage = Storage::open(&project).unwrap();
    let bytes = fs::read(&input).unwrap();
    let first = storage.put_asset(&bytes).unwrap();
    let second = storage.put_asset(&bytes).unwrap();
    assert_eq!(first, second);
    assert_eq!(fs::read_dir(project.join("assets")).unwrap().count(), 1);
    let before = storage.manifest(100_000).unwrap();
    let error = storage::with_failure("asset_write", || {
        storage.put_asset(b"a different immutable result asset")
    })
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::IoError);
    assert_eq!(fs::read_dir(project.join("assets")).unwrap().count(), 1);
    assert_eq!(storage.asset(&first, 100_000).unwrap(), bytes);
    assert_eq!(storage.manifest(100_000).unwrap(), before);
    open(&project)
        .restore(None, &mut Diagnostics::default())
        .unwrap();
}

#[test]
fn undo_redo_account_for_all_commit_bytes_before_replacing_manifest() {
    let (dir, input, project) = fixture();
    create(&input, &project);
    apply(
        &project,
        &pipeline(dir.path(), vec![OperationSpec::identity(); 10]),
        "r0",
    )
    .unwrap();
    Project::undo(
        &project,
        "r10",
        &ResourceLimits::default(),
        &mut Diagnostics::default(),
    )
    .unwrap();
    let manifest_path = project.join("manifest.json");
    let original = fs::read(&manifest_path).unwrap();
    let exact_budget = open(&project).history.metadata_bytes;
    let tight = ResourceLimits {
        max_project_bytes: exact_budget,
        ..ResourceLimits::default()
    };
    // r0 -> r10 grows the pointer by one byte, beyond the total metadata budget.
    let error = Project::redo(&project, "r0", &tight, &mut Diagnostics::default()).unwrap_err();
    assert_eq!(error.code, ErrorCode::ResourceLimit);
    assert_eq!(fs::read(&manifest_path).unwrap(), original);
    Project::open(&project, &tight, &mut Diagnostics::default()).unwrap();

    // Valid compact JSON is accepted, but cursor writes must budget pretty-serialization too.
    Project::redo(
        &project,
        "r0",
        &ResourceLimits::default(),
        &mut Diagnostics::default(),
    )
    .unwrap();
    for redo in [false, true] {
        let manifest: Manifest =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        let compact = serde_json::to_vec(&manifest).unwrap();
        fs::write(&manifest_path, &compact).unwrap();
        let tight = ResourceLimits {
            max_project_bytes: open(&project).history.metadata_bytes,
            ..ResourceLimits::default()
        };
        let result = if redo {
            Project::redo(&project, "r0", &tight, &mut Diagnostics::default())
        } else {
            Project::undo(&project, "r10", &tight, &mut Diagnostics::default())
        };
        assert_eq!(result.unwrap_err().code, ErrorCode::ResourceLimit);
        assert_eq!(fs::read(&manifest_path).unwrap(), compact);
        Project::open(&project, &tight, &mut Diagnostics::default()).unwrap();
        if !redo {
            Project::undo(
                &project,
                "r10",
                &ResourceLimits::default(),
                &mut Diagnostics::default(),
            )
            .unwrap();
        }
    }
}

#[test]
fn failed_ops_and_manifest_publication_leave_only_unreferenced_records() {
    let (dir, input, project) = fixture();
    create(&input, &project);
    let pipeline = pipeline(dir.path(), vec![OperationSpec::identity(); 2]);
    let before = fs::read(project.join("manifest.json")).unwrap();
    for stage in ["ops_write", "manifest_write", "manifest_publish"] {
        let error = storage::with_failure(stage, || apply(&project, &pipeline, "r0")).unwrap_err();
        assert_eq!(error.code, ErrorCode::IoError, "{stage}");
        assert_eq!(fs::read(project.join("manifest.json")).unwrap(), before);
        let loaded = open(&project);
        assert!(loaded.commits().is_empty());
        assert_eq!(
            loaded
                .restore(Some("r1"), &mut Diagnostics::default())
                .err()
                .unwrap()
                .code,
            ErrorCode::RevisionNotFound
        );
        assert_eq!(
            loaded
                .restore(None, &mut Diagnostics::default())
                .unwrap()
                .revision
                .0,
            "r0"
        );
        for folder in [&project, &project.join("ops")] {
            assert!(fs::read_dir(folder).unwrap().all(|entry| {
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".pic-tmp-")
            }));
        }
    }
    // Orphans are ignored, including malformed files not named in manifest.
    fs::write(project.join("ops/unpublished.json"), b"not a commit").unwrap();
    assert_eq!(apply(&project, &pipeline, "r0").unwrap().revision.0, "r2");
    assert_eq!(open(&project).commits().len(), 1);
}

#[test]
fn cursor_publication_failures_keep_history_and_head() {
    let (dir, input, project) = fixture();
    create(&input, &project);
    apply(
        &project,
        &pipeline(dir.path(), vec![OperationSpec::identity()]),
        "r0",
    )
    .unwrap();
    let before = fs::read(project.join("manifest.json")).unwrap();
    let error = storage::with_failure("manifest_publish", || {
        Project::undo(
            &project,
            "r1",
            &ResourceLimits::default(),
            &mut Diagnostics::default(),
        )
    })
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::IoError);
    assert_eq!(fs::read(project.join("manifest.json")).unwrap(), before);
    Project::undo(
        &project,
        "r1",
        &ResourceLimits::default(),
        &mut Diagnostics::default(),
    )
    .unwrap();
    let before = fs::read(project.join("manifest.json")).unwrap();
    assert!(
        storage::with_failure("manifest_write", || Project::redo(
            &project,
            "r0",
            &ResourceLimits::default(),
            &mut Diagnostics::default()
        ))
        .is_err()
    );
    assert_eq!(fs::read(project.join("manifest.json")).unwrap(), before);
}

#[test]
fn final_publication_rechecks_revision_and_manifest_even_after_an_aba_cursor_move() {
    let (dir, input, path) = fixture();
    create(&input, &path);
    let snapshot = open(&path);
    apply(
        &path,
        &pipeline(dir.path(), vec![OperationSpec::identity()]),
        "r0",
    )
    .unwrap();
    assert_eq!(
        snapshot
            .publish(&snapshot.manifest_bytes, "r0")
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    Project::undo(
        &path,
        "r1",
        &ResourceLimits::default(),
        &mut Diagnostics::default(),
    )
    .unwrap();
    assert_eq!(
        snapshot
            .publish(&snapshot.manifest_bytes, "r0")
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(open(&path).commits().len(), 1);
    // A previously opened read snapshot is still internally consistent.
    assert_eq!(
        snapshot
            .restore(None, &mut Diagnostics::default())
            .unwrap()
            .revision
            .0,
        "r0"
    );
}

#[test]
fn every_float_sample_matches_direct_pipeline_after_cross_commit_replay() {
    let (dir, input, project) = fixture();
    create(&input, &project);
    let ops = [
        Operation::Adjust(AdjustParams {
            exposure: 3.123456789012345,
            brightness: -0.125,
            contrast: 1.33,
            saturation: 0.7,
        })
        .to_spec(),
        Operation::Curves(CurvesParams {
            channel: Channel::Red,
            points: vec![
                [0.0, 0.0],
                [0.12345678901234568, 0.29876543210987654],
                [1.0, 1.0],
            ],
        })
        .to_spec(),
        Operation::Resize(ResizeParams {
            width: Some(11),
            height: None,
            filter: Interpolation::Bilinear,
        })
        .to_spec(),
        Operation::Adjust(AdjustParams {
            exposure: -3.123456789012345,
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
        })
        .to_spec(),
    ];
    let limits = ResourceLimits::default();
    let input_raster = codec::load(&input, &limits, &mut Diagnostics::default())
        .unwrap()
        .raster;
    for (index, op) in ops.iter().enumerate() {
        apply(
            &project,
            &pipeline(dir.path(), vec![op.clone()]),
            &format!("r{index}"),
        )
        .unwrap();
        let direct = pipeline(dir.path(), ops[..=index].to_vec())
            .execute(input_raster.clone())
            .unwrap()
            .raster;
        let replayed = open(&project)
            .restore(None, &mut Diagnostics::default())
            .unwrap()
            .raster;
        assert_eq!(
            (direct.width(), direct.height()),
            (replayed.width(), replayed.height())
        );
        for (a, b) in direct.pixels().iter().zip(replayed.pixels()) {
            assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits));
        }
    }
    let loaded = open(&project);
    let high = loaded
        .restore(Some("r1"), &mut Diagnostics::default())
        .unwrap();
    assert!(high.raster.pixels().iter().any(|pixel| pixel[0] > 1.0));
    assert!(high.raster.pixels().iter().any(|pixel| pixel[0] < 0.0));
}

#[test]
fn metadata_and_history_limits_fail_before_publication() {
    let (dir, input, project) = fixture();
    create(&input, &project);
    let steps = pipeline(dir.path(), vec![OperationSpec::identity(); 2]);
    let before = fs::read(project.join("manifest.json")).unwrap();
    for limits in [
        ResourceLimits {
            max_history_operations: 1,
            ..ResourceLimits::default()
        },
        ResourceLimits {
            max_project_bytes: before.len() as u64 + 10,
            ..ResourceLimits::default()
        },
    ] {
        let error = Project::apply(
            ApplyRequest {
                project: &project,
                pipeline: &steps,
                expected_revision: "r0",
                revision: None,
            },
            &limits,
            &mut Diagnostics::default(),
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::ResourceLimit);
        assert_eq!(fs::read(project.join("manifest.json")).unwrap(), before);
    }
    assert_eq!(fs::read_dir(project.join("ops")).unwrap().count(), 0);
}

#[test]
fn version_integrity_and_graph_validation_reject_malformed_committed_records() {
    let (dir, input, project) = fixture();
    create(&input, &project);
    apply(
        &project,
        &pipeline(dir.path(), vec![OperationSpec::identity(); 2]),
        "r0",
    )
    .unwrap();
    let manifest_path = project.join("manifest.json");
    let original_manifest: Manifest =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    let original_commit: Commit = serde_json::from_slice(
        &fs::read(
            project
                .join("ops")
                .join(format!("{}.json", original_manifest.commits[0].sha256)),
        )
        .unwrap(),
    )
    .unwrap();
    for (case, expected) in [
        (0, ErrorCode::UnsupportedVersion),
        (1, ErrorCode::UnsupportedVersion),
        (2, ErrorCode::InvalidProject),
        (3, ErrorCode::InvalidProject),
        (4, ErrorCode::InvalidProject),
        (5, ErrorCode::InvalidTarget),
    ] {
        let mut manifest = original_manifest.clone();
        let mut commit = original_commit.clone();
        match case {
            0 => commit.schema_version = 99,
            1 => commit.steps[1].operation.op_version = 99,
            2 => commit.steps[1].base_revision = RevisionId("r2".into()),
            3 => commit.steps[1].op_id = "op1".into(),
            4 => manifest.current_revision = RevisionId("r1".into()),
            5 => commit.steps[0].operation.target = TargetId("renamed canvas".into()),
            _ => unreachable!(),
        }
        let bytes = serde_json::to_vec(&commit).unwrap();
        let digest = storage::hash(&bytes);
        fs::write(project.join("ops").join(format!("{digest}.json")), bytes).unwrap();
        manifest.commits[0].sha256 = digest;
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert_eq!(
            Project::open(
                &project,
                &ResourceLimits::default(),
                &mut Diagnostics::default()
            )
            .err()
            .unwrap()
            .code,
            expected,
            "case {case}"
        );
    }
}
