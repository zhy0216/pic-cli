use super::*;
#[allow(dead_code)]
#[path = "../../examples/support/layer_milestone.rs"]
mod milestone;

#[test]
fn background_subject_external_mask_text_save_reopen_modify_export_milestone() {
    let dir = tempfile::tempdir().unwrap();
    let report = milestone::run(&binary(), dir.path());
    assert_eq!(report["logical_steps"], 10);
}

#[test]
fn group_clip_adjustment_commands_and_direct_pipeline_have_identical_export_pixels() {
    let dir = fixture();
    let dir = dir.path();
    create(dir);
    let group = success(
        dir,
        &[
            "project",
            "group",
            "add",
            "work.pic",
            "--id",
            "g",
            "--width",
            "8",
            "--height",
            "6",
            "--expect-revision",
            "r0",
        ],
    );
    assert_eq!(group["steps"][0]["operation"]["op"], "group_add");
    success(
        dir,
        &[
            "project",
            "layer",
            "parent",
            "work.pic",
            "--target",
            "base",
            "--parent",
            "g",
            "--expect-revision",
            "r1",
        ],
    );
    success(
        dir,
        &[
            "project",
            "adjustment",
            "add",
            "work.pic",
            "--id",
            "a",
            "--width",
            "8",
            "--height",
            "6",
            "--op",
            "invert",
            "--expect-revision",
            "r2",
        ],
    );
    success(
        dir,
        &[
            "project",
            "layer",
            "parent",
            "work.pic",
            "--target",
            "a",
            "--parent",
            "g",
            "--expect-revision",
            "r3",
        ],
    );
    success(
        dir,
        &[
            "project",
            "layer",
            "clip",
            "work.pic",
            "--target",
            "a",
            "--base",
            "base",
            "--expect-revision",
            "r4",
        ],
    );
    success(
        dir,
        &[
            "project",
            "adjustment",
            "set",
            "work.pic",
            "--target",
            "a",
            "--op",
            "grayscale",
            "--expect-revision",
            "r5",
        ],
    );
    success(
        dir,
        &["project", "export", "work.pic", "--output", "project.png"],
    );
    let state = success(dir, &["project", "inspect", "work.pic"]);
    let ops: Vec<_> = state["commits"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|c| {
            c["steps"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s["operation"].clone())
        })
        .collect();
    write_pipeline(dir, "all.json", ops);
    success(
        dir,
        &[
            "run",
            "--input",
            "input.png",
            "--pipeline",
            "all.json",
            "--output",
            "direct.png",
        ],
    );
    assert_eq!(
        pixels(&dir.join("project.png")),
        pixels(&dir.join("direct.png"))
    );
    success(
        dir,
        &[
            "project",
            "preview",
            "work.pic",
            "--target",
            "a",
            "--output",
            "adjustment.png",
        ],
    );
    assert_eq!(
        pixels(&dir.join("project.png")),
        pixels(&dir.join("adjustment.png"))
    );
    let manifest = manifest_bytes(dir);
    for (args, code) in [
        (
            vec![
                "project",
                "layer",
                "parent",
                "work.pic",
                "--target",
                "g",
                "--parent",
                "g",
                "--expect-revision",
                "r6",
            ],
            "dependency_cycle",
        ),
        (
            vec![
                "project",
                "layer",
                "clip",
                "work.pic",
                "--target",
                "base",
                "--base",
                "a",
                "--expect-revision",
                "r6",
            ],
            "dependency_cycle",
        ),
        (
            vec![
                "project",
                "layer",
                "remove",
                "work.pic",
                "--target",
                "base",
                "--expect-revision",
                "r6",
            ],
            "invalid_hierarchy",
        ),
        (
            vec![
                "project",
                "layer",
                "set",
                "work.pic",
                "--target",
                "a",
                "--blend",
                "multiply",
                "--expect-revision",
                "r6",
            ],
            "invalid_argument",
        ),
        (
            vec![
                "project",
                "adjustment",
                "set",
                "work.pic",
                "--target",
                "a",
                "--op",
                "blur",
                "--params",
                r#"{"sigma":1}"#,
                "--expect-revision",
                "r6",
            ],
            "invalid_argument",
        ),
    ] {
        failure(dir, &args, code);
        assert_eq!(manifest_bytes(dir), manifest);
    }
    success(
        dir,
        &[
            "project",
            "layer",
            "clip",
            "work.pic",
            "--target",
            "a",
            "--expect-revision",
            "r6",
        ],
    );
    assert!(
        success(dir, &["project", "inspect", "work.pic"])["document"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["id"] == "a")
            .unwrap()["clip"]
            .is_null()
    );
}

#[test]
fn text_add_help_and_missing_glyph_failure_are_truthful_and_atomic() {
    let dir = fixture();
    let dir = dir.path();
    create(dir);
    fs::write(
        dir.join("font.ttf"),
        include_bytes!("../../../../tests/fonts/DejaVuSans.ttf"),
    )
    .unwrap();
    let added = success(
        dir,
        &[
            "project",
            "text",
            "add",
            "work.pic",
            "--id",
            "t",
            "--font",
            "font.ttf",
            "--text",
            "é",
            "--width",
            "64",
            "--height",
            "32",
            "--expect-revision",
            "r0",
        ],
    );
    assert_eq!(added["steps"][0]["operation"]["op"], "text_add");
    failure(
        dir,
        &[
            "project",
            "template-export",
            "work.pic",
            "--output",
            "text-template.json",
        ],
        "unsupported_template",
    );
    assert!(!dir.join("text-template.json").exists());
    let before = manifest_bytes(dir);
    failure(
        dir,
        &[
            "project",
            "text",
            "set",
            "work.pic",
            "--target",
            "t",
            "--font",
            "font.ttf",
            "--text",
            "中",
            "--width",
            "64",
            "--height",
            "32",
            "--expect-revision",
            "r1",
        ],
        "missing_glyph",
    );
    assert_eq!(manifest_bytes(dir), before);
    failure(
        dir,
        &[
            "project",
            "text",
            "set",
            "work.pic",
            "--target",
            "t",
            "--font",
            "gone.ttf",
            "--text",
            "A",
            "--width",
            "64",
            "--height",
            "32",
            "--expect-revision",
            "r1",
        ],
        "file_not_found",
    );
    assert_eq!(manifest_bytes(dir), before);
    for args in [
        vec!["project", "text", "add", "--help"],
        vec!["project", "group", "add", "--help"],
        vec!["project", "adjustment", "add", "--help"],
        vec!["project", "layer", "clip", "--help"],
    ] {
        assert_eq!(call(dir, &args)["ok"], true);
    }
}
