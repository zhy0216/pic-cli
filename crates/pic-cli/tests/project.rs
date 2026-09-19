use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use image::{Rgba, RgbaImage};
use serde_json::{Value, json};

#[path = "project/layers.rs"]
mod layers;
#[path = "project/replay.rs"]
mod replay;

fn binary() -> PathBuf {
    std::env::var_os("PIC_CLI_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_pic-cli")))
}

fn call(dir: &Path, args: &[&str]) -> Value {
    let output = Command::new(binary())
        .current_dir(dir)
        .arg("--json")
        .args(args)
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{args:?}: {error}; {output:?}"));
    assert_eq!(value["ok"], output.status.success(), "{value}");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(String::from_utf8_lossy(&output.stdout).lines().count(), 1);
    assert!(output.stderr.is_empty(), "{output:?}");
    value
}

fn success(dir: &Path, args: &[&str]) -> Value {
    let value = call(dir, args);
    assert_eq!(value["ok"], true, "{args:?}: {value}");
    value["data"].clone()
}

fn failure(dir: &Path, args: &[&str], code: &str) {
    let value = call(dir, args);
    assert_eq!(value["ok"], false, "{args:?}: {value}");
    assert_eq!(value["error"]["code"], code, "{args:?}: {value}");
    assert!(value["data"].is_null());
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    RgbaImage::from_fn(8, 6, |x, y| {
        Rgba([(x * 31) as u8, (y * 47) as u8, 173, (x * 35) as u8])
    })
    .save(dir.path().join("input.png"))
    .unwrap();
    write_pipeline(dir.path(), "identity.json", vec![op("identity", json!({}))]);
    dir
}

fn op(name: &str, params: Value) -> Value {
    json!({"op":name,"op_version":1,"target":"canvas","params":params})
}

fn adjust(exposure: f64) -> Value {
    op(
        "adjust",
        json!({"exposure":exposure,"brightness":0.0,"contrast":1.0,"saturation":1.0}),
    )
}

fn write_pipeline(dir: &Path, name: &str, operations: Vec<Value>) {
    fs::write(
        dir.join(name),
        serde_json::to_vec(&json!({"schema_version":1,"operations":operations})).unwrap(),
    )
    .unwrap();
}

fn create(dir: &Path) -> Value {
    success(
        dir,
        &[
            "project",
            "create",
            "--input",
            "input.png",
            "--output",
            "work.pic",
        ],
    )
}

fn apply(dir: &Path, pipeline: &str, expected: &str) -> Value {
    success(
        dir,
        &[
            "project",
            "apply",
            "work.pic",
            "--pipeline",
            pipeline,
            "--expect-revision",
            expected,
        ],
    )
}

fn manifest_bytes(dir: &Path) -> Vec<u8> {
    fs::read(dir.join("work.pic/manifest.json")).unwrap()
}
fn manifest(dir: &Path) -> Value {
    serde_json::from_slice(&manifest_bytes(dir)).unwrap()
}
fn pixels(path: &Path) -> RgbaImage {
    image::open(path).unwrap().into_rgba8()
}

#[test]
fn groups_intermediate_steps_undo_redo_forks_and_exports_across_processes() {
    let dir = fixture();
    let dir = dir.path();
    assert_eq!(create(dir)["revision"], "r0");
    let group = vec![
        op("crop", json!({"x":1,"y":1,"width":6,"height":4})),
        adjust(2.0),
        op("resize", json!({"width":9,"filter":"bilinear"})),
    ];
    write_pipeline(dir, "group.json", group.clone());
    let applied = apply(dir, "group.json", "r0");
    assert_eq!(applied["commit_id"], "c1");
    assert_eq!(applied["base_revision"], "r0");
    assert_eq!(applied["revision"], "r3");
    assert_eq!(applied["steps"].as_array().unwrap().len(), 3);
    assert_eq!(
        applied["steps"][2]["operation"]["params"]["height"],
        Value::Null
    );
    for n in 1..=3 {
        let inspected = success(
            dir,
            &[
                "project",
                "inspect",
                "work.pic",
                "--revision",
                &format!("r{n}"),
            ],
        );
        assert_eq!(inspected["revision"], format!("r{n}"));
        assert_eq!(inspected["op_id"], format!("op{n}"));
        assert_eq!(inspected["width"], if n == 3 { 9 } else { 6 });
        assert_eq!(inspected["operations_replayed"], n);
    }
    let undo = success(
        dir,
        &["project", "undo", "work.pic", "--expect-revision", "r3"],
    );
    assert_eq!(undo["revision"], "r0");
    assert!(undo["steps"].as_array().unwrap().is_empty());
    assert_eq!(
        success(
            dir,
            &["project", "redo", "work.pic", "--expect-revision", "r0"]
        )["revision"],
        "r3"
    );
    write_pipeline(dir, "finish.json", vec![adjust(-2.0)]);
    assert_eq!(apply(dir, "finish.json", "r3")["revision"], "r4");
    let original_manifest = manifest_bytes(dir);
    let old_ops: Vec<_> = fs::read_dir(dir.join("work.pic/ops"))
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect();
    success(
        dir,
        &[
            "project",
            "preview",
            "work.pic",
            "--revision",
            "r2",
            "--output",
            "preview.png",
        ],
    );
    success(
        dir,
        &["project", "export", "work.pic", "--output", "final.png"],
    );
    assert_eq!(manifest_bytes(dir), original_manifest);
    let mut direct = group;
    direct.push(adjust(-2.0));
    write_pipeline(dir, "direct.json", direct);
    success(
        dir,
        &[
            "run",
            "--input",
            "input.png",
            "--pipeline",
            "direct.json",
            "--output",
            "direct.png",
        ],
    );
    assert_eq!(
        pixels(&dir.join("final.png")),
        pixels(&dir.join("direct.png"))
    );

    write_pipeline(
        dir,
        "fork.json",
        vec![op("flip", json!({"axis":"horizontal"}))],
    );
    let fork = success(
        dir,
        &[
            "project",
            "apply",
            "work.pic",
            "--pipeline",
            "fork.json",
            "--revision",
            "r1",
            "--expect-revision",
            "r4",
        ],
    );
    assert_eq!(fork["revision"], "r5");
    assert_eq!(fork["base_revision"], "r1");
    assert_eq!(fork["previous_revision"], "r4");
    let inspected = success(dir, &["project", "inspect", "work.pic"]);
    assert_eq!(inspected["group_boundaries"], json!(["r0", "r1", "r5"]));
    assert_eq!(inspected["commits"].as_array().unwrap().len(), 3);
    success(
        dir,
        &[
            "project",
            "export",
            "work.pic",
            "--revision",
            "r4",
            "--output",
            "old.png",
        ],
    );
    assert_eq!(pixels(&dir.join("old.png")), pixels(&dir.join("final.png")));
    for (path, bytes) in old_ops {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    for (command, expected, target) in [
        ("undo", "r5", "r1"),
        ("undo", "r1", "r0"),
        ("redo", "r0", "r1"),
        ("redo", "r1", "r5"),
    ] {
        assert_eq!(
            success(
                dir,
                &[
                    "project",
                    command,
                    "work.pic",
                    "--expect-revision",
                    expected
                ]
            )["revision"],
            target
        );
    }
    failure(
        dir,
        &["project", "redo", "work.pic", "--expect-revision", "r5"],
        "history_boundary",
    );
    failure(
        dir,
        &[
            "project",
            "apply",
            "work.pic",
            "--pipeline",
            "identity.json",
            "--revision",
            "r1",
            "--expect-revision",
            "r1",
        ],
        "revision_conflict",
    );
}

#[test]
fn editing_after_undo_discards_redo_but_keeps_old_revisions_readable() {
    let dir = fixture();
    let dir = dir.path();
    create(dir);
    apply(dir, "identity.json", "r0");
    apply(dir, "identity.json", "r1");
    success(
        dir,
        &["project", "undo", "work.pic", "--expect-revision", "r2"],
    );
    assert_eq!(apply(dir, "identity.json", "r1")["revision"], "r3");
    success(
        dir,
        &["project", "undo", "work.pic", "--expect-revision", "r3"],
    );
    assert_eq!(
        success(
            dir,
            &["project", "redo", "work.pic", "--expect-revision", "r1"]
        )["revision"],
        "r3"
    );
    assert_eq!(
        success(dir, &["project", "inspect", "work.pic", "--revision", "r2"])["revision"],
        "r2"
    );
}

#[test]
fn moved_project_needs_neither_original_image_nor_pipeline_files() {
    let dir = fixture();
    let dir = dir.path();
    create(dir);
    write_pipeline(dir, "edit.json", vec![adjust(2.0), adjust(-2.0)]);
    apply(dir, "edit.json", "r0");
    let expected = pixels(&dir.join("input.png"));
    assert_eq!(
        fs::read_dir(dir.join("work.pic/assets")).unwrap().count(),
        1
    );
    fs::create_dir(dir.join("relocated")).unwrap();
    fs::rename(dir.join("work.pic"), dir.join("relocated/moved.pic")).unwrap();
    fs::remove_file(dir.join("input.png")).unwrap();
    fs::remove_file(dir.join("edit.json")).unwrap();
    fs::remove_file(dir.join("identity.json")).unwrap();
    success(
        &dir.join("relocated"),
        &["project", "export", "moved.pic", "--output", "result.png"],
    );
    assert_eq!(pixels(&dir.join("relocated/result.png")), expected);
    success(
        &dir.join("relocated"),
        &["project", "inspect", "moved.pic", "--revision", "r1"],
    );
}

#[test]
fn failed_operations_queries_and_preview_never_enter_history() {
    let dir = fixture();
    let dir = dir.path();
    create(dir);
    write_pipeline(
        dir,
        "invalid.json",
        vec![
            adjust(1.0),
            op("crop", json!({"x":7,"y":0,"width":8,"height":6})),
        ],
    );
    write_pipeline(dir, "empty.json", vec![]);
    let mut unsupported = op("identity", json!({}));
    unsupported["op_version"] = json!(99);
    write_pipeline(dir, "unsupported.json", vec![unsupported]);
    let before = manifest_bytes(dir);
    for (file, code) in [
        ("invalid.json", "invalid_argument"),
        ("empty.json", "invalid_argument"),
        ("unsupported.json", "unsupported_version"),
    ] {
        failure(
            dir,
            &[
                "project",
                "apply",
                "work.pic",
                "--pipeline",
                file,
                "--expect-revision",
                "r0",
            ],
            code,
        );
    }
    failure(
        dir,
        &[
            "project",
            "apply",
            "work.pic",
            "--pipeline",
            "identity.json",
            "--revision",
            "r404",
            "--expect-revision",
            "r0",
        ],
        "revision_not_found",
    );
    failure(
        dir,
        &["project", "undo", "work.pic", "--expect-revision", "r0"],
        "history_boundary",
    );
    success(dir, &["project", "inspect", "work.pic"]);
    success(
        dir,
        &["project", "preview", "work.pic", "--output", "preview.png"],
    );
    assert_eq!(manifest_bytes(dir), before);
    assert_eq!(fs::read_dir(dir.join("work.pic/ops")).unwrap().count(), 0);
    assert_eq!(
        pixels(&dir.join("preview.png")),
        pixels(&dir.join("input.png"))
    );
}

#[test]
fn missing_corrupt_assets_and_unknown_project_versions_report_clear_errors() {
    let dir = fixture();
    let dir = dir.path();
    create(dir);
    let original = manifest(dir);
    let asset = dir
        .join("work.pic/assets")
        .join(original["source"]["sha256"].as_str().unwrap());
    let bytes = fs::read(&asset).unwrap();
    fs::remove_file(&asset).unwrap();
    failure(dir, &["project", "inspect", "work.pic"], "asset_missing");
    let mut corrupt = bytes.clone();
    corrupt[20] ^= 1;
    fs::write(&asset, corrupt).unwrap();
    failure(
        dir,
        &["project", "export", "work.pic", "--output", "bad.png"],
        "integrity_mismatch",
    );
    assert!(!dir.join("bad.png").exists());
    fs::write(asset, bytes).unwrap();
    for (field, value) in [
        ("schema_version", json!(99)),
        ("pixel_semantics", json!("future_color_v2")),
    ] {
        let mut changed = original.clone();
        changed[field] = value;
        fs::write(
            dir.join("work.pic/manifest.json"),
            serde_json::to_vec(&changed).unwrap(),
        )
        .unwrap();
        failure(
            dir,
            &["project", "inspect", "work.pic"],
            "unsupported_version",
        );
    }
}

#[test]
fn corrupted_or_missing_committed_ops_are_not_silently_skipped() {
    let dir = fixture();
    let dir = dir.path();
    create(dir);
    apply(dir, "identity.json", "r0");
    let manifest = manifest(dir);
    let path = dir.join("work.pic/ops").join(format!(
        "{}.json",
        manifest["commits"][0]["sha256"].as_str().unwrap()
    ));
    let mut bytes = fs::read(&path).unwrap();
    bytes[5] ^= 1;
    fs::write(&path, bytes).unwrap();
    failure(
        dir,
        &["project", "inspect", "work.pic"],
        "integrity_mismatch",
    );
    fs::remove_file(path).unwrap();
    failure(dir, &["project", "inspect", "work.pic"], "file_not_found");
}

#[test]
fn project_paths_reject_traversal_symlinks_and_outputs_inside_authoritative_storage() {
    use std::os::unix::fs::symlink;
    let dir = fixture();
    let dir = dir.path();
    create(dir);
    let original = manifest(dir);
    for malicious in ["../../input.png", "/tmp/input.png", "..\\input.png"] {
        let mut value = original.clone();
        value["source"]["sha256"] = json!(malicious);
        fs::write(
            dir.join("work.pic/manifest.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        failure(dir, &["project", "inspect", "work.pic"], "unsafe_path");
    }
    fs::write(
        dir.join("work.pic/manifest.json"),
        serde_json::to_vec(&original).unwrap(),
    )
    .unwrap();
    let source = dir
        .join("work.pic/assets")
        .join(original["source"]["sha256"].as_str().unwrap());
    let bytes = fs::read(&source).unwrap();
    fs::remove_file(&source).unwrap();
    symlink(dir.join("input.png"), &source).unwrap();
    failure(dir, &["project", "inspect", "work.pic"], "unsafe_path");
    fs::remove_file(&source).unwrap();
    fs::write(source, bytes).unwrap();
    fs::rename(dir.join("work.pic/assets"), dir.join("outside-assets")).unwrap();
    symlink(dir.join("outside-assets"), dir.join("work.pic/assets")).unwrap();
    failure(dir, &["project", "inspect", "work.pic"], "unsafe_path");
    fs::remove_file(dir.join("work.pic/assets")).unwrap();
    fs::rename(dir.join("outside-assets"), dir.join("work.pic/assets")).unwrap();
    symlink(dir.join("work.pic"), dir.join("alias.pic")).unwrap();
    failure(dir, &["project", "inspect", "alias.pic"], "unsafe_path");
    let before = manifest_bytes(dir);
    failure(
        dir,
        &[
            "project",
            "export",
            "work.pic",
            "--output",
            "work.pic/manifest.json",
            "--format",
            "png",
            "--overwrite",
        ],
        "unsafe_path",
    );
    failure(
        dir,
        &[
            "project",
            "preview",
            "work.pic",
            "--output",
            "work.pic/assets/preview.png",
        ],
        "unsafe_path",
    );
    assert_eq!(manifest_bytes(dir), before);
}

#[test]
fn export_failures_preserve_existing_output_and_project() {
    let dir = fixture();
    let dir = dir.path();
    create(dir);
    apply(dir, "identity.json", "r0");
    let before = manifest_bytes(dir);
    fs::write(dir.join("existing.jpg"), b"original export").unwrap();
    failure(
        dir,
        &["project", "export", "work.pic", "--output", "existing.jpg"],
        "output_exists",
    );
    failure(
        dir,
        &[
            "project",
            "export",
            "work.pic",
            "--output",
            "existing.jpg",
            "--overwrite",
        ],
        "alpha_not_supported",
    );
    assert_eq!(
        fs::read(dir.join("existing.jpg")).unwrap(),
        b"original export"
    );
    assert_eq!(manifest_bytes(dir), before);
    success(
        dir,
        &[
            "project",
            "export",
            "work.pic",
            "--output",
            "flattened.jpg",
            "--jpeg-background",
            "#ffffff",
            "--jpeg-quality",
            "95",
        ],
    );
    assert_eq!(pixels(&dir.join("flattened.jpg")).dimensions(), (8, 6));
}

#[test]
fn simultaneous_expected_revision_writers_have_exactly_one_winner() {
    use rustix::fs::{FlockOperation, flock};
    let dir = fixture();
    let dir = dir.path();
    create(dir);
    let lock = fs::File::open(dir.join("work.pic/.lock")).unwrap();
    flock(&lock, FlockOperation::LockExclusive).unwrap();
    let mut children: Vec<_> = (0..2)
        .map(|_| {
            Command::new(binary())
                .current_dir(dir)
                .args([
                    "--json",
                    "project",
                    "apply",
                    "work.pic",
                    "--pipeline",
                    "identity.json",
                    "--expect-revision",
                    "r0",
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    // Both real CLI writers wait on the lock; snapshot readers do not need it.
    assert_eq!(
        success(dir, &["project", "inspect", "work.pic"])["revision"],
        "r0"
    );
    for child in &mut children {
        assert!(child.try_wait().unwrap().is_none());
    }
    drop(lock);
    let values: Vec<Value> = children
        .into_iter()
        .map(|child| {
            let output = child.wait_with_output().unwrap();
            serde_json::from_slice(&output.stdout).unwrap()
        })
        .collect();
    assert_eq!(
        values.iter().filter(|value| value["ok"] == true).count(),
        1,
        "{values:?}"
    );
    assert_eq!(
        values
            .iter()
            .filter(|value| value["error"]["code"] == "revision_conflict")
            .count(),
        1,
        "{values:?}"
    );
    let inspected = success(dir, &["project", "inspect", "work.pic"]);
    assert_eq!(inspected["revision"], "r1");
    assert_eq!(inspected["commits"].as_array().unwrap().len(), 1);
    assert_eq!(fs::read_dir(dir.join("work.pic/ops")).unwrap().count(), 1);
}

#[test]
fn simultaneous_creators_never_replace_the_winning_project() {
    let dir = fixture();
    let dir = dir.path();
    let children: Vec<_> = (0..2)
        .map(|_| {
            Command::new(binary())
                .current_dir(dir)
                .args([
                    "--json",
                    "project",
                    "create",
                    "--input",
                    "input.png",
                    "--output",
                    "work.pic",
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let values: Vec<Value> = children
        .into_iter()
        .map(|child| serde_json::from_slice(&child.wait_with_output().unwrap().stdout).unwrap())
        .collect();
    assert_eq!(
        values.iter().filter(|value| value["ok"] == true).count(),
        1,
        "{values:?}"
    );
    assert_eq!(
        values
            .iter()
            .filter(|value| value["error"]["code"] == "output_exists")
            .count(),
        1,
        "{values:?}"
    );
    success(
        dir,
        &["project", "export", "work.pic", "--output", "result.png"],
    );
    assert_eq!(
        pixels(&dir.join("result.png")),
        pixels(&dir.join("input.png"))
    );
    assert!(fs::read_dir(dir).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".pic-create-")
    }));
}
