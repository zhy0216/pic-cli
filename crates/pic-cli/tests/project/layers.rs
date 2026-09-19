use super::*;

fn layer_op(op_name: &str, target: &str, params: Value) -> Value {
    json!({"op":op_name,"op_version":1,"target":target,"params":params})
}
fn transform(x: f64, y: f64, degrees: f64) -> Value {
    json!({"x":x,"y":y,"scale_x":1.0,"scale_y":1.0,"degrees":degrees,"flip_x":false,"flip_y":false,"filter":"nearest"})
}
fn setup() -> tempfile::TempDir {
    let dir = fixture();
    RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 0]))
        .save(dir.path().join("input.png"))
        .unwrap();
    RgbaImage::from_raw(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 255])
        .unwrap()
        .save(dir.path().join("subject.png"))
        .unwrap();
    image::GrayImage::from_raw(2, 1, vec![128, 255])
        .unwrap()
        .save(dir.path().join("mask.png"))
        .unwrap();
    create(dir.path());
    dir
}
fn map(matrix: &Value, p: [f64; 2]) -> [f64; 2] {
    let v: Vec<f64> = matrix
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    [
        v[0] * p[0] + v[2] * p[1] + v[4],
        v[1] * p[0] + v[3] * p[1] + v[5],
    ]
}

#[test]
fn layers_masks_selection_checkpoint_move_undo_redo_and_continue_across_processes() {
    let dir = setup();
    let dir = dir.path();
    success(
        dir,
        &[
            "project",
            "layer",
            "add",
            "work.pic",
            "--id",
            "subject",
            "--source",
            "subject.png",
            "--expect-revision",
            "r0",
        ],
    );
    success(
        dir,
        &[
            "project",
            "layer",
            "transform",
            "work.pic",
            "--target",
            "subject",
            "--x",
            "3",
            "--y",
            "1",
            "--degrees",
            "90",
            "--filter",
            "nearest",
            "--expect-revision",
            "r1",
        ],
    );
    success(
        dir,
        &[
            "project",
            "mask",
            "set",
            "work.pic",
            "--target",
            "subject",
            "--source",
            "mask.png",
            "--expect-revision",
            "r2",
        ],
    );
    success(
        dir,
        &[
            "project",
            "selection",
            "set",
            "work.pic",
            "--space",
            "canvas",
            "--region",
            "2,1,1,1",
            "--expect-revision",
            "r3",
        ],
    );
    let applied = success(
        dir,
        &[
            "project",
            "layer",
            "edit",
            "work.pic",
            "--target",
            "subject",
            "--op",
            "invert",
            "--expect-revision",
            "r4",
        ],
    );
    assert_eq!(applied["revision"], "r5");
    let checkpoint = success(dir, &["project", "checkpoint", "work.pic"]);
    assert_eq!(checkpoint["disk_stored"], true);
    let inspected = success(dir, &["project", "inspect", "work.pic"]);
    assert_eq!(inspected["replay"]["cache_hit"]["tier"], "disk");
    assert_eq!(inspected["document"]["layers"].as_array().unwrap().len(), 2);
    assert_eq!(inspected["document"]["layers"][1]["id"], "subject");
    assert_eq!(inspected["document"]["layers"][1]["mask"], "mask:subject");
    assert_eq!(inspected["document"]["selection"]["space"], "canvas");
    success(
        dir,
        &["project", "export", "work.pic", "--output", "full.png"],
    );
    let full = pixels(&dir.join("full.png"));
    assert_eq!(full.get_pixel(2, 1).0, [0, 255, 255, 128]);
    assert_eq!(full.get_pixel(2, 2).0, [0, 255, 0, 255]);
    success(
        dir,
        &[
            "project",
            "preview",
            "work.pic",
            "--target",
            "subject",
            "--output",
            "layer.png",
        ],
    );
    assert_eq!(pixels(&dir.join("layer.png")), full);
    let preview = success(
        dir,
        &[
            "project",
            "preview",
            "work.pic",
            "--target",
            "subject",
            "--region",
            "2,1,1,2",
            "--width",
            "2",
            "--height",
            "4",
            "--filter",
            "nearest",
            "--output",
            "region.png",
        ],
    );
    assert_eq!(
        map(&preview["coordinates"]["preview_to_layer"], [1.0, 1.0]),
        [0.5, 0.5]
    );
    assert_eq!(
        map(&preview["coordinates"]["layer_to_preview"], [0.5, 0.5]),
        [1.0, 1.0]
    );
    assert_eq!(
        map(&preview["coordinates"]["layer_to_canvas"], [0.5, 0.5]),
        [2.5, 1.5]
    );
    let mask = success(
        dir,
        &[
            "project",
            "preview",
            "work.pic",
            "--target",
            "mask:subject",
            "--region",
            "2,1,1,2",
            "--output",
            "mask-preview.png",
        ],
    );
    assert_eq!(
        pixels(&dir.join("mask-preview.png")).get_pixel(0, 0).0,
        [128, 128, 128, 255]
    );
    assert_eq!(
        mask["coordinates"]["canvas_to_layer"],
        preview["coordinates"]["canvas_to_layer"]
    );
    let cached = success(
        dir,
        &[
            "project",
            "preview",
            "work.pic",
            "--target",
            "mask:subject",
            "--region",
            "2,1,1,2",
            "--output",
            "mask-cached.png",
        ],
    );
    assert_eq!(cached["replay"]["cache_hit"]["kind"], "preview");
    assert_eq!(cached["coordinates"], mask["coordinates"]);
    for file in ["input.png", "subject.png", "mask.png", "identity.json"] {
        fs::remove_file(dir.join(file)).unwrap();
    }
    fs::create_dir(dir.join("moved")).unwrap();
    fs::rename(dir.join("work.pic"), dir.join("moved/work.pic")).unwrap();
    let moved = dir.join("moved");
    let continued = success(
        &moved,
        &[
            "project",
            "layer",
            "set",
            "work.pic",
            "--target",
            "subject",
            "--opacity",
            "0.5",
            "--expect-revision",
            "r5",
        ],
    );
    assert_eq!(continued["replay"]["cache_hit"]["revision"], "r5");
    assert_eq!(continued["revision"], "r6");
    success(
        &moved,
        &["project", "undo", "work.pic", "--expect-revision", "r6"],
    );
    let undo = success(&moved, &["project", "inspect", "work.pic"]);
    assert_eq!(undo["document"], inspected["document"]);
    success(
        &moved,
        &["project", "redo", "work.pic", "--expect-revision", "r5"],
    );
    success(
        &moved,
        &["project", "export", "work.pic", "--output", "continued.png"],
    );
    assert_eq!(
        pixels(&moved.join("continued.png")).get_pixel(2, 1).0,
        [0, 255, 255, 64]
    );
    success(&moved, &["project", "cache-clear", "work.pic"]);
    let replay = success(
        &moved,
        &["project", "export", "work.pic", "--output", "replayed.png"],
    );
    assert_eq!(replay["operations_replayed"], 6);
    assert_eq!(
        pixels(&moved.join("continued.png")),
        pixels(&moved.join("replayed.png"))
    );
    success(
        &moved,
        &[
            "project",
            "selection",
            "clear",
            "work.pic",
            "--expect-revision",
            "r6",
        ],
    );
    success(
        &moved,
        &[
            "project",
            "mask",
            "remove",
            "work.pic",
            "--target",
            "subject",
            "--expect-revision",
            "r7",
        ],
    );
    success(
        &moved,
        &[
            "project",
            "layer",
            "edit",
            "work.pic",
            "--target",
            "subject",
            "--op",
            "crop",
            "--params",
            r#"{"x":0,"y":0,"width":1,"height":1}"#,
            "--expect-revision",
            "r8",
        ],
    );
    let current = success(&moved, &["project", "inspect", "work.pic"]);
    assert_eq!(current["document"]["layers"][1]["width"], 1);
    assert!(current["document"]["layers"][1]["mask"].is_null());
    assert_eq!(current["document"]["layers"][0]["width"], 4);
}

#[test]
fn composite_command_json_pipeline_and_layer_project_export_share_pixels() {
    let dir = setup();
    let dir = dir.path();
    for mode in ["normal", "multiply", "screen", "overlay"] {
        // Opaque colored backdrop exercises the blend term, with half coverage and opacity.
        RgbaImage::from_pixel(4, 4, Rgba([64, 128, 192, 160]))
            .save(dir.join("backdrop.png"))
            .unwrap();
        let project = format!("{mode}.pic");
        success(
            dir,
            &[
                "project",
                "create",
                "--input",
                "backdrop.png",
                "--output",
                &project,
            ],
        );
        let command = format!("command-{mode}.png");
        let direct = format!("direct-{mode}.png");
        let exported = format!("project-{mode}.png");
        success(
            dir,
            &[
                "composite",
                "--input",
                "backdrop.png",
                "--overlay",
                "subject.png",
                "--mask",
                "mask.png",
                "--x",
                "1",
                "--y",
                "2",
                "--opacity",
                "0.5",
                "--blend",
                mode,
                "--filter",
                "nearest",
                "--output",
                &command,
            ],
        );
        fs::create_dir_all(dir.join("pipelines")).unwrap();
        write_pipeline(
            &dir.join("pipelines"),
            "composite.json",
            vec![op(
                "composite",
                json!({"source":"../subject.png","mask":"../mask.png","opacity":0.5,"blend":mode,"transform":transform(1.0,2.0,0.0)}),
            )],
        );
        success(
            dir,
            &[
                "run",
                "--input",
                "backdrop.png",
                "--pipeline",
                "pipelines/composite.json",
                "--output",
                &direct,
            ],
        );
        write_pipeline(
            dir,
            "layers.json",
            vec![
                op(
                    "layer_add",
                    json!({"id":"subject","source":"subject.png","name":"Same"}),
                ),
                layer_op("mask_set", "subject", json!({"source":"mask.png"})),
                layer_op("layer_transform", "subject", transform(1.0, 2.0, 0.0)),
                layer_op("layer_set", "subject", json!({"opacity":0.5,"blend":mode})),
            ],
        );
        success(
            dir,
            &[
                "project",
                "apply",
                &project,
                "--pipeline",
                "layers.json",
                "--expect-revision",
                "r0",
            ],
        );
        success(dir, &["project", "export", &project, "--output", &exported]);
        assert_eq!(pixels(&dir.join(&command)), pixels(&dir.join(&direct)));
        assert_eq!(pixels(&dir.join(&direct)), pixels(&dir.join(exported)));
    }
}

#[test]
fn layer_failures_and_stale_revision_never_publish_partial_history_or_output() {
    let dir = setup();
    let dir = dir.path();
    success(
        dir,
        &[
            "project",
            "layer",
            "add",
            "work.pic",
            "--id",
            "subject",
            "--source",
            "subject.png",
            "--expect-revision",
            "r0",
        ],
    );
    let before = manifest_bytes(dir);
    for (args, code) in [
        (
            vec![
                "project",
                "layer",
                "add",
                "work.pic",
                "--id",
                "subject",
                "--source",
                "subject.png",
                "--expect-revision",
                "r1",
            ],
            "invalid_target",
        ),
        (
            vec![
                "project",
                "layer",
                "set",
                "work.pic",
                "--target",
                "missing",
                "--visible",
                "false",
                "--expect-revision",
                "r1",
            ],
            "invalid_target",
        ),
        (
            vec![
                "project",
                "layer",
                "reorder",
                "work.pic",
                "--target",
                "subject",
                "--before",
                "absent",
                "--expect-revision",
                "r1",
            ],
            "invalid_target",
        ),
        (
            vec![
                "project",
                "mask",
                "set",
                "work.pic",
                "--target",
                "subject",
                "--source",
                "input.png",
                "--expect-revision",
                "r1",
            ],
            "invalid_mask",
        ),
        (
            vec![
                "project",
                "layer",
                "transform",
                "work.pic",
                "--target",
                "subject",
                "--scale-x",
                "0",
                "--expect-revision",
                "r1",
            ],
            "invalid_argument",
        ),
        (
            vec![
                "project",
                "layer",
                "edit",
                "work.pic",
                "--target",
                "canvas",
                "--op",
                "invert",
                "--expect-revision",
                "r1",
            ],
            "invalid_target",
        ),
        (
            vec![
                "project",
                "selection",
                "set",
                "work.pic",
                "--space",
                "absent",
                "--region",
                "0,0,1,1",
                "--expect-revision",
                "r1",
            ],
            "invalid_target",
        ),
        (
            vec![
                "project",
                "layer",
                "set",
                "work.pic",
                "--target",
                "subject",
                "--visible",
                "false",
                "--expect-revision",
                "r0",
            ],
            "revision_conflict",
        ),
        (
            vec![
                "project",
                "preview",
                "work.pic",
                "--target",
                "mask:subject",
                "--output",
                "absent.png",
            ],
            "invalid_target",
        ),
    ] {
        failure(dir, &args, code);
        assert_eq!(manifest_bytes(dir), before);
        assert!(!dir.join("absent.png").exists());
    }
    write_pipeline(
        dir,
        "failed.json",
        vec![
            layer_op("layer_set", "subject", json!({"visible":false})),
            layer_op("layer_remove", "absent", json!({})),
        ],
    );
    failure(
        dir,
        &[
            "project",
            "apply",
            "work.pic",
            "--pipeline",
            "failed.json",
            "--expect-revision",
            "r1",
        ],
        "invalid_target",
    );
    assert_eq!(manifest_bytes(dir), before);
}

#[test]
fn multilayer_order_ids_and_canvas_crop_survive_revision_changes() {
    let dir = setup();
    let dir = dir.path();
    write_pipeline(
        dir,
        "add.json",
        vec![
            op(
                "layer_add",
                json!({"id":"a","source":"subject.png","name":"duplicate"}),
            ),
            op(
                "layer_add",
                json!({"id":"b","source":"subject.png","name":"duplicate"}),
            ),
            layer_op("layer_transform", "a", transform(2.0, 1.0, 90.0)),
            layer_op("layer_transform", "b", transform(1.0, 0.0, 0.0)),
        ],
    );
    apply(dir, "add.json", "r0");
    success(
        dir,
        &[
            "project",
            "layer",
            "reorder",
            "work.pic",
            "--target",
            "b",
            "--before",
            "a",
            "--expect-revision",
            "r4",
        ],
    );
    success(
        dir,
        &[
            "project",
            "layer",
            "remove",
            "work.pic",
            "--target",
            "b",
            "--expect-revision",
            "r5",
        ],
    );
    write_pipeline(
        dir,
        "crop.json",
        vec![op("crop", json!({"x":1,"y":1,"width":2,"height":2}))],
    );
    apply(dir, "crop.json", "r6");
    success(dir, &["project", "checkpoint", "work.pic"]);
    let state = success(dir, &["project", "inspect", "work.pic"]);
    assert_eq!(state["width"], 2);
    assert_eq!(state["document"]["layers"][1]["id"], "a");
    assert_eq!(state["document"]["layers"][1]["width"], 2);
    assert_eq!(state["document"]["layers"][1]["height"], 1);
    success(
        dir,
        &[
            "project",
            "revise",
            "work.pic",
            "--step-revision",
            "r3",
            "--params",
            &serde_json::to_string(&transform(3.0, 1.0, 90.0)).unwrap(),
            "--expect-revision",
            "r7",
        ],
    );
    let revised = success(dir, &["project", "inspect", "work.pic"]);
    assert_ne!(revised["revision"], "r7");
    assert_eq!(revised["document"]["layers"][1]["transform"]["x"], 2.0);
    let old = success(dir, &["project", "inspect", "work.pic", "--revision", "r7"]);
    assert_eq!(old["document"], state["document"]);
    failure(
        dir,
        &[
            "project",
            "template-export",
            "work.pic",
            "--output",
            "unsupported.json",
        ],
        "unsupported_template",
    );
    assert!(!dir.join("unsupported.json").exists());
}

#[test]
fn cached_layer_previews_require_assets_and_layer_templates_fail_closed() {
    let dir = setup();
    let dir = dir.path();
    write_pipeline(
        dir,
        "add.json",
        vec![
            op(
                "layer_add",
                json!({"id":"a","source":"subject.png","name":"a"}),
            ),
            layer_op("mask_set", "a", json!({"source":"mask.png"})),
        ],
    );
    apply(dir, "add.json", "r0");
    success(dir, &["project", "checkpoint", "work.pic"]);
    success(
        dir,
        &[
            "project",
            "preview",
            "work.pic",
            "--target",
            "a",
            "--output",
            "cached.png",
        ],
    );
    let inspected = success(dir, &["project", "inspect", "work.pic"]);
    for step in [0, 1] {
        let asset = inspected["commits"][0]["steps"][step]["input_assets"][1]["sha256"]
            .as_str()
            .unwrap();
        let path = dir.join("work.pic/assets").join(asset);
        let bytes = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        failure(
            dir,
            &[
                "project",
                "preview",
                "work.pic",
                "--target",
                "a",
                "--output",
                "missing.png",
            ],
            "asset_missing",
        );
        failure(dir, &["project", "inspect", "work.pic"], "asset_missing");
        assert!(!dir.join("missing.png").exists());
        fs::write(path, bytes).unwrap();
    }
    failure(
        dir,
        &[
            "project",
            "template-export",
            "work.pic",
            "--output",
            "template.json",
        ],
        "unsupported_template",
    );
    let template = json!({"schema_version":1,"pixel_semantics":"linear_srgb_rgba32f_straight_v1","input_slot":"input","target_slots":["canvas"],"operations":[{"op":"layer_add","op_version":1,"target_slot":"canvas","params_slot":"step_1","suggested_params":{"id":"old","source":"subject.png","name":"Old"}}]});
    let bindings = json!({"schema_version":1,"inputs":{"input":"input.png"},"targets":{"canvas":"canvas"},"params":{"step_1":{"id":"old","source":"subject.png","name":"Old"}}});
    fs::write(
        dir.join("template.json"),
        serde_json::to_vec(&template).unwrap(),
    )
    .unwrap();
    fs::write(
        dir.join("bindings.json"),
        serde_json::to_vec(&bindings).unwrap(),
    )
    .unwrap();
    failure(
        dir,
        &[
            "project",
            "template-run",
            "--template",
            "template.json",
            "--bindings",
            "bindings.json",
            "--output",
            "forbidden.pic",
        ],
        "unsupported_template",
    );
    assert!(!dir.join("forbidden.pic").exists());
}

#[test]
fn reduced_mask_preview_resamples_coverage_before_display_gamma() {
    let dir = setup();
    let dir = dir.path();
    image::GrayImage::from_raw(2, 1, vec![0, 255])
        .unwrap()
        .save(dir.join("mask.png"))
        .unwrap();
    write_pipeline(
        dir,
        "mask.json",
        vec![
            op(
                "layer_add",
                json!({"id":"a","source":"subject.png","name":"a"}),
            ),
            layer_op("mask_set", "a", json!({"source":"mask.png"})),
        ],
    );
    apply(dir, "mask.json", "r0");
    success(
        dir,
        &[
            "project",
            "preview",
            "work.pic",
            "--target",
            "mask:a",
            "--region",
            "0,0,2,1",
            "--width",
            "1",
            "--height",
            "1",
            "--output",
            "coverage.png",
        ],
    );
    success(
        dir,
        &[
            "project",
            "preview",
            "work.pic",
            "--target",
            "a",
            "--region",
            "0,0,2,1",
            "--width",
            "1",
            "--height",
            "1",
            "--output",
            "color.png",
        ],
    );
    let coverage = pixels(&dir.join("coverage.png")).get_pixel(0, 0).0;
    assert_eq!(coverage, [128, 128, 128, 255]);
    assert_eq!(
        pixels(&dir.join("color.png")).get_pixel(0, 0)[3],
        coverage[0]
    );
}
