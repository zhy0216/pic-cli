use super::*;

#[test]
fn p0_checkpoint_intermediate_preview_continue_export_and_clear_replay_in_new_processes() {
    let dir = fixture();
    let dir = dir.path();
    create(dir);
    let mut operations = vec![
        op("crop", json!({"x":1,"y":1,"width":6,"height":4})),
        adjust(3.25),
    ];
    write_pipeline(dir, "edit.json", operations.clone());
    apply(dir, "edit.json", "r0");
    let authority = manifest_bytes(dir);
    let checkpoint = success(
        dir,
        &["project", "checkpoint", "work.pic", "--revision", "r2"],
    );
    assert_eq!(checkpoint["disk_stored"], true);
    assert_eq!(checkpoint["canvas"], json!({"width":6,"height":4}));
    let preview_args = [
        "project",
        "preview",
        "work.pic",
        "--revision",
        "r1",
        "--region",
        "1,1,4,2",
        "--width",
        "2",
        "--output",
        "preview.png",
        "--overwrite",
    ];
    let preview = success(dir, &preview_args);
    assert_eq!(preview["revision"], "r1");
    assert_eq!(preview["op_id"], "op1");
    assert_eq!(preview["canvas"], json!({"width":6,"height":4}));
    assert_eq!(preview["preview_size"], json!({"width":2,"height":1}));
    assert_eq!(
        preview["coordinates"]["preview_to_canvas"],
        json!({"scale":[2.0,2.0],"offset":[1.0,1.0]})
    );
    assert_eq!(
        preview["coordinates"]["canvas_to_preview"],
        json!({"scale":[0.5,0.5],"offset":[-0.5,-0.5]})
    );
    assert_eq!(manifest_bytes(dir), authority);
    let hit = success(dir, &preview_args);
    assert_eq!(hit["replay"]["cache_hit"]["kind"], "preview");
    assert_eq!(hit["replay"]["cache_hit"]["tier"], "disk");
    assert_eq!(hit["operations_replayed"], 0);
    // The same new-process request safely rebuilds a corrupt preview.
    let key = hit["replay"]["cache_hit"]["key"].as_str().unwrap();
    fs::write(dir.join("work.pic/cache").join(format!("{key}.bin")), []).unwrap();
    assert_eq!(success(dir, &preview_args)["operations_replayed"], 1);
    failure(
        dir,
        &[
            "project",
            "apply",
            "work.pic",
            "--pipeline",
            "identity.json",
            "--expect-revision",
            "r1",
        ],
        "revision_conflict",
    );
    write_pipeline(dir, "finish.json", vec![adjust(-3.25)]);
    let continued = apply(dir, "finish.json", "r2");
    assert_eq!(continued["replay"]["cache_hit"]["revision"], "r2");
    assert_eq!(continued["replay"]["recomputed_revisions"], json!(["r3"]));
    let exported = success(
        dir,
        &["project", "export", "work.pic", "--output", "final.png"],
    );
    assert_eq!(exported["width"], 6);
    assert_eq!(exported["height"], 4);
    assert_eq!(exported["replay"]["cache_hit"]["revision"], "r2");
    operations.push(adjust(-3.25));
    write_pipeline(dir, "direct.json", operations);
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
    let authority = manifest_bytes(dir);
    // Import paths are not a fallback; full replay after clear uses embedded source bytes.
    fs::remove_file(dir.join("input.png")).unwrap();
    let cleared = success(dir, &["project", "cache-clear", "work.pic"]);
    assert!(cleared["removed_bytes"].as_u64().unwrap() > 0);
    for folder in ["cache", "checkpoints"] {
        assert_eq!(
            fs::read_dir(dir.join("work.pic").join(folder))
                .unwrap()
                .count(),
            0
        );
    }
    let replayed = success(
        dir,
        &["project", "export", "work.pic", "--output", "replayed.png"],
    );
    assert_eq!(replayed["operations_replayed"], 3);
    assert!(replayed["replay"]["cache_hit"].is_null());
    assert_eq!(
        pixels(&dir.join("replayed.png")),
        pixels(&dir.join("final.png"))
    );
    assert_eq!(manifest_bytes(dir), authority);
    // Every committed step remains observable, including the initial state and middle of a group.
    for (revision, width, height) in [("r0", 8, 6), ("r1", 6, 4), ("r2", 6, 4), ("r3", 6, 4)] {
        let value = success(
            dir,
            &[
                "project",
                "preview",
                "work.pic",
                "--revision",
                revision,
                "--output",
                "step.png",
                "--overwrite",
            ],
        );
        assert_eq!(value["canvas"], json!({"width":width,"height":height}));
    }
}

#[test]
fn twenty_step_parameter_revisions_reuse_only_valid_prefixes_across_processes() {
    let dir = fixture();
    let dir = dir.path();
    create(dir);
    let mut ops = vec![adjust(0.05); 20];
    write_pipeline(dir, "twenty.json", ops.clone());
    apply(dir, "twenty.json", "r0");
    let original_ops: Vec<_> = fs::read_dir(dir.join("work.pic/ops"))
        .unwrap()
        .map(|e| {
            let path = e.unwrap().path();
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect();
    success(
        dir,
        &["project", "export", "work.pic", "--output", "original.png"],
    );
    for revision in ["r2", "r17", "r20"] {
        success(
            dir,
            &["project", "checkpoint", "work.pic", "--revision", revision],
        );
    }
    let late = success(
        dir,
        &[
            "project",
            "revise",
            "work.pic",
            "--step-revision",
            "r18",
            "--params",
            &adjust(0.75)["params"].to_string(),
            "--expect-revision",
            "r20",
        ],
    );
    assert_eq!(late["revision"], "r23");
    assert_eq!(late["replay"]["reused_steps"], 17);
    assert_eq!(late["replay"]["cache_hit"]["revision"], "r17");
    assert_eq!(
        late["replay"]["recomputed_revisions"],
        json!(["r21", "r22", "r23"])
    );
    ops[17] = adjust(0.75);
    success(dir, &["project", "checkpoint", "work.pic"]);
    let early = success(
        dir,
        &[
            "project",
            "revise",
            "work.pic",
            "--step-revision",
            "r3",
            "--params",
            &adjust(-0.3)["params"].to_string(),
            "--expect-revision",
            "r23",
        ],
    );
    assert_eq!(early["revision"], "r41");
    assert_eq!(early["replay"]["reused_steps"], 2);
    assert_eq!(early["replay"]["cache_hit"]["revision"], "r2");
    assert_eq!(
        early["replay"]["recomputed_revisions"]
            .as_array()
            .unwrap()
            .len(),
        18
    );
    ops[2] = adjust(-0.3);
    write_pipeline(dir, "direct.json", ops);
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
    success(
        dir,
        &["project", "export", "work.pic", "--output", "revised.png"],
    );
    assert_eq!(
        pixels(&dir.join("revised.png")),
        pixels(&dir.join("direct.png"))
    );
    success(
        dir,
        &[
            "project",
            "export",
            "work.pic",
            "--revision",
            "r20",
            "--output",
            "old.png",
        ],
    );
    assert_eq!(
        pixels(&dir.join("old.png")),
        pixels(&dir.join("original.png"))
    );
    for (path, bytes) in original_ops {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    assert_eq!(
        success(
            dir,
            &["project", "undo", "work.pic", "--expect-revision", "r41"]
        )["revision"],
        "r2"
    );
    assert_eq!(
        success(
            dir,
            &["project", "redo", "work.pic", "--expect-revision", "r2"]
        )["revision"],
        "r41"
    );
}

#[test]
fn templates_require_explicit_rebinding_and_create_atomic_independent_history() {
    let dir = fixture();
    let dir = dir.path();
    create(dir);
    let old_ops = vec![
        op("crop", json!({"x":1,"y":1,"width":6,"height":4})),
        adjust(1.5),
        op("resize", json!({"width":12,"filter":"bilinear"})),
    ];
    write_pipeline(dir, "edit.json", old_ops);
    apply(dir, "edit.json", "r0");
    success(dir, &["project", "checkpoint", "work.pic"]);
    let old_source = manifest(dir)["source"]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let exported = success(
        dir,
        &[
            "project",
            "template-export",
            "work.pic",
            "--output",
            "template.json",
        ],
    );
    let template = exported["template"].clone();
    assert_eq!(template["input_slot"], "input");
    assert_eq!(template["target_slots"], json!(["canvas"]));
    let text = fs::read_to_string(dir.join("template.json")).unwrap();
    for forbidden in ["revision", "sha256", "op_id", "result_assets", "input.png"] {
        assert!(!text.contains(forbidden));
    }
    RgbaImage::from_fn(10, 8, |x, y| {
        Rgba([(x * 20) as u8, (y * 30) as u8, 90, 180])
    })
    .save(dir.join("new.png"))
    .unwrap();
    fs::create_dir(dir.join("bindings")).unwrap();
    let new_ops = vec![
        op("crop", json!({"x":2,"y":3,"width":5,"height":3})),
        adjust(-0.25),
        op("resize", json!({"width":10,"height":6,"filter":"nearest"})),
    ];
    let binding = json!({
        "schema_version":1, "inputs":{"input":"../new.png"}, "targets":{"canvas":"canvas"},
        "params":{"step_1":new_ops[0]["params"],"step_2":new_ops[1]["params"],"step_3":new_ops[2]["params"]}
    });
    let bind_path = dir.join("bindings/bind.json");
    let run_args = [
        "project",
        "template-run",
        "--template",
        "template.json",
        "--bindings",
        "bindings/bind.json",
        "--output",
        "new.pic",
    ];
    for (case, code) in [
        (0, "invalid_argument"),
        (1, "invalid_argument"),
        (2, "invalid_argument"),
        (3, "invalid_json"),
        (4, "invalid_json"),
        (5, "invalid_argument"),
    ] {
        let mut changed = binding.clone();
        match case {
            0 => changed["targets"] = json!({}),
            1 => {
                changed["params"].as_object_mut().unwrap().remove("step_1");
            }
            2 => changed["inputs"] = json!({}),
            3 => changed["revision"] = json!("r3"),
            4 => changed["assets"] = json!({"mask":"old-mask.png"}),
            5 => changed["params"]["step_1"]["x"] = json!(99), // runtime failure after staging
            _ => unreachable!(),
        }
        fs::write(&bind_path, changed.to_string()).unwrap();
        failure(dir, &run_args, code);
        assert!(!dir.join("new.pic").exists());
        assert!(fs::read_dir(dir).unwrap().all(|e| {
            !e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".pic-template-")
        }));
    }
    fs::write(&bind_path, binding.to_string()).unwrap();
    // Unsupported content-dependent results/ops cannot be smuggled into a template.
    for (case, code) in [
        (0, "invalid_json"),
        (1, "unknown_operation"),
        (2, "invalid_argument"),
        (3, "unsupported_version"),
    ] {
        let mut changed = template.clone();
        match case {
            0 => changed["operations"][0]["result_assets"] = json!([{"sha256":old_source}]),
            1 => changed["operations"][0]["op"] = json!("model_result"),
            2 => changed["operations"][0]["suggested_params"]["mask"] = json!("old-mask.png"),
            3 => changed["pixel_semantics"] = json!("future_color"),
            _ => unreachable!(),
        }
        fs::write(dir.join("template.json"), changed.to_string()).unwrap();
        failure(dir, &run_args, code);
        assert!(!dir.join("new.pic").exists());
    }
    fs::write(dir.join("template.json"), template.to_string()).unwrap();
    fs::remove_dir_all(dir.join("work.pic")).unwrap();
    fs::remove_file(dir.join("input.png")).unwrap();
    let created = success(dir, &run_args);
    assert_eq!(created["previous_revision"], "r0");
    assert_eq!(created["base_revision"], "r0");
    assert_eq!(created["revision"], "r3"); // IDs are local to the newly created project.
    assert_eq!(
        created["steps"][0]["operation"]["params"],
        new_ops[0]["params"]
    );
    assert_eq!(created["replay"]["reused_steps"], 0);
    assert!(created["replay"]["cache_hit"].is_null());
    let fresh = success(dir, &["project", "inspect", "new.pic"]);
    assert_ne!(fresh["manifest"]["source"]["sha256"], old_source);
    assert_eq!(fresh["commits"].as_array().unwrap().len(), 1);
    success(
        dir,
        &["project", "export", "new.pic", "--output", "new-export.png"],
    );
    write_pipeline(dir, "direct-new.json", new_ops);
    success(
        dir,
        &[
            "run",
            "--input",
            "new.png",
            "--pipeline",
            "direct-new.json",
            "--output",
            "direct-new.png",
        ],
    );
    assert_eq!(
        pixels(&dir.join("new-export.png")),
        pixels(&dir.join("direct-new.png"))
    );
    let bytes = fs::read(dir.join("new.pic/manifest.json")).unwrap();
    failure(dir, &run_args, "output_exists");
    assert_eq!(fs::read(dir.join("new.pic/manifest.json")).unwrap(), bytes);
}

#[test]
fn preview_rejects_invalid_target_region_and_size_without_publishing() {
    let dir = fixture();
    let dir = dir.path();
    create(dir);
    let before = manifest_bytes(dir);
    fs::write(dir.join("keep.png"), b"keep existing").unwrap();
    for (extra, code) in [
        (vec!["--target", "layer-old"], "invalid_target"),
        (vec!["--region", "7,0,2,2"], "invalid_argument"),
        (vec!["--width", "0"], "invalid_argument"),
        (vec!["--width", "4294967295"], "resource_limit"),
        (vec!["--revision", "r404"], "revision_not_found"),
    ] {
        let mut args = vec![
            "project",
            "preview",
            "work.pic",
            "--output",
            "keep.png",
            "--overwrite",
        ];
        args.extend(extra);
        failure(dir, &args, code);
        assert_eq!(fs::read(dir.join("keep.png")).unwrap(), b"keep existing");
        assert_eq!(manifest_bytes(dir), before);
    }
}

#[test]
fn concurrent_checkpoint_publishers_share_disk_budget_and_clear_zero_length_entries() {
    let dir = fixture();
    let dir = dir.path();
    create(dir);
    write_pipeline(dir, "edits.json", vec![adjust(0.1); 3]);
    apply(dir, "edits.json", "r0");
    let authority = manifest_bytes(dir);
    let children: Vec<_> = ["r1", "r2", "r3"]
        .into_iter()
        .map(|revision| {
            Command::new(binary())
                .current_dir(dir)
                .args([
                    "--json",
                    "project",
                    "checkpoint",
                    "work.pic",
                    "--revision",
                    revision,
                    "--cache-disk-bytes",
                    "888",
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        let output = child.wait_with_output().unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(output.status.success(), "{value}");
        assert_eq!(value["data"]["disk_stored"], true);
    }
    let bytes: u64 = ["cache", "checkpoints"]
        .into_iter()
        .flat_map(|name| fs::read_dir(dir.join("work.pic").join(name)).unwrap())
        .map(|e| e.unwrap().metadata().unwrap().len())
        .sum();
    assert!(bytes <= 888);
    for folder in ["cache", "checkpoints"] {
        fs::write(
            dir.join("work.pic")
                .join(folder)
                .join(format!("{}.bin", "0".repeat(64))),
            [],
        )
        .unwrap();
    }
    success(dir, &["project", "cache-clear", "work.pic"]);
    for folder in ["cache", "checkpoints"] {
        assert_eq!(
            fs::read_dir(dir.join("work.pic").join(folder))
                .unwrap()
                .count(),
            0
        );
    }
    assert_eq!(manifest_bytes(dir), authority);
    assert_eq!(
        fs::read_dir(dir.join("work.pic/assets")).unwrap().count(),
        1
    );
    assert_eq!(
        success(dir, &["project", "inspect", "work.pic"])["operations_replayed"],
        3
    );
}
