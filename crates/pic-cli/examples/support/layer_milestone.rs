//! Reused by the CLI integration test and the release acceptance example.
use image::{Rgba, RgbaImage};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command, time::Instant};

pub fn call(binary: &Path, dir: &Path, args: &[&str]) -> Value {
    let output = Command::new(binary)
        .current_dir(dir)
        .arg("--json")
        .args(args)
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("{args:?}: {e}; {output:?}"));
    assert_eq!(value["ok"], output.status.success());
    assert!(output.stderr.is_empty(), "{output:?}");
    value
}
pub fn success(binary: &Path, dir: &Path, args: &[&str]) -> Value {
    let value = call(binary, dir, args);
    assert_eq!(value["ok"], true, "{args:?}: {value}");
    value["data"].clone()
}
fn pixels(path: &Path) -> RgbaImage {
    image::open(path).unwrap().into_rgba8()
}
fn op(name: &str, target: &str, params: Value) -> Value {
    json!({"op":name,"op_version":1,"target":target,"params":params})
}
fn transform(x: u32, y: u32) -> Value {
    json!({"x":x,"y":y,"scale_x":1,"scale_y":1,"degrees":0,"flip_x":false,"flip_y":false,"filter":"nearest"})
}
fn text(font: &str, content: &str, align: &str) -> Value {
    json!({"font":font,"text":content,"size":24,"line_height":28,"width":128,"height":64,"align":align,"color":[255,255,255,255]})
}
fn write_pipeline(dir: &Path, file: &str, operations: Vec<Value>) {
    fs::write(
        dir.join(file),
        serde_json::to_vec_pretty(&json!({"schema_version":1,"operations":operations})).unwrap(),
    )
    .unwrap();
}

pub fn run(binary: &Path, dir: &Path) -> Value {
    let started = Instant::now();
    RgbaImage::from_fn(192, 128, |_, y| {
        if y < 32 {
            Rgba([32, 64, 96, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    })
    .save(dir.join("background.png"))
    .unwrap();
    RgbaImage::from_pixel(32, 24, Rgba([255, 0, 0, 255]))
        .save(dir.join("subject.png"))
        .unwrap();
    image::GrayImage::from_fn(32, 24, |x, _| {
        image::Luma([if x < 8 {
            0
        } else if x < 16 {
            128
        } else {
            255
        }])
    })
    .save(dir.join("external-mask.png"))
    .unwrap();
    fs::write(
        dir.join("font.ttf"),
        include_bytes!("../../../../tests/fonts/DejaVuSans.ttf"),
    )
    .unwrap();
    fs::write(
        dir.join("LICENSE-DejaVu.txt"),
        include_bytes!("../../../../tests/fonts/LICENSE-DejaVu.txt"),
    )
    .unwrap();
    let original_text = "Café ffi\nΩμέγα";
    let edited_text = "Office é\nПривет";
    let operations = vec![
        op(
            "group_add",
            "canvas",
            json!({"id":"layout","name":"Layout","width":184,"height":120}),
        ),
        op(
            "layer_add",
            "canvas",
            json!({"id":"subject","name":"Subject","source":"subject.png"}),
        ),
        op(
            "layer_parent",
            "subject",
            json!({"parent":"layout","before":null}),
        ),
        op("mask_set", "subject", json!({"source":"external-mask.png"})),
        op("layer_transform", "subject", transform(16, 16)),
        op("layer_transform", "layout", transform(8, 8)),
        op(
            "text_add",
            "canvas",
            json!({"id":"caption","name":"Caption","text":text("font.ttf",original_text,"center")}),
        ),
        op(
            "layer_parent",
            "caption",
            json!({"parent":"layout","before":null}),
        ),
        op("layer_transform", "caption", transform(48, 56)),
    ];
    write_pipeline(dir, "before.json", operations.clone());
    let mut after_ops = operations;
    after_ops.push(op(
        "text_set",
        "caption",
        text("font.ttf", edited_text, "right"),
    ));
    write_pipeline(dir, "after.json", after_ops);
    for (pipeline, output) in [
        ("before.json", "before-direct.png"),
        ("after.json", "after-direct.png"),
    ] {
        success(
            binary,
            dir,
            &[
                "run",
                "--input",
                "background.png",
                "--pipeline",
                pipeline,
                "--output",
                output,
            ],
        );
    }
    success(
        binary,
        dir,
        &[
            "project",
            "create",
            "--input",
            "background.png",
            "--output",
            "work.pic",
        ],
    );
    let change = success(
        binary,
        dir,
        &[
            "project",
            "apply",
            "work.pic",
            "--pipeline",
            "before.json",
            "--expect-revision",
            "r0",
        ],
    );
    assert_eq!(change["revision"], "r9");
    assert_eq!(change["steps"].as_array().unwrap().len(), 9);
    assert!(
        success(binary, dir, &["project", "checkpoint", "work.pic"])["disk_stored"]
            .as_bool()
            .unwrap()
    );
    let before_state = success(binary, dir, &["project", "inspect", "work.pic"]);
    assert_eq!(before_state["replay"]["reused_steps"], 9);
    success(
        binary,
        dir,
        &[
            "project",
            "preview",
            "work.pic",
            "--output",
            "before-preview.png",
        ],
    );
    success(
        binary,
        dir,
        &[
            "project",
            "export",
            "work.pic",
            "--output",
            "before-export.png",
        ],
    );
    assert_eq!(
        pixels(&dir.join("before-direct.png")),
        pixels(&dir.join("before-export.png"))
    );
    assert_eq!(
        pixels(&dir.join("before-preview.png")),
        pixels(&dir.join("before-export.png"))
    );
    let font = before_state["document"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["id"] == "caption")
        .unwrap()["kind"]["params"]["font"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(font.starts_with("asset:"));
    // Persisted project must remain editable after all imported resources disappear.
    for file in [
        "background.png",
        "subject.png",
        "external-mask.png",
        "font.ttf",
        "before.json",
        "after.json",
    ] {
        fs::remove_file(dir.join(file)).unwrap();
    }
    fs::create_dir(dir.join("relocated")).unwrap();
    fs::rename(dir.join("work.pic"), dir.join("relocated/work.pic")).unwrap();
    let moved = dir.join("relocated");
    let changed = success(
        binary,
        &moved,
        &[
            "project",
            "text",
            "set",
            "work.pic",
            "--target",
            "caption",
            "--font",
            &font,
            "--text",
            edited_text,
            "--size",
            "24",
            "--line-height",
            "28",
            "--width",
            "128",
            "--height",
            "64",
            "--align",
            "right",
            "--color",
            "#ffffffff",
            "--expect-revision",
            "r9",
        ],
    );
    assert_eq!(changed["revision"], "r10");
    assert_eq!(changed["replay"]["reused_steps"], 9);
    success(
        binary,
        &moved,
        &["project", "undo", "work.pic", "--expect-revision", "r10"],
    );
    let undo = success(binary, &moved, &["project", "inspect", "work.pic"]);
    assert_eq!(undo["document"], before_state["document"]);
    success(
        binary,
        &moved,
        &["project", "export", "work.pic", "--output", "undo.png"],
    );
    assert_eq!(
        pixels(&moved.join("undo.png")),
        pixels(&dir.join("before-direct.png"))
    );
    success(
        binary,
        &moved,
        &["project", "redo", "work.pic", "--expect-revision", "r9"],
    );
    success(
        binary,
        &moved,
        &[
            "project",
            "preview",
            "work.pic",
            "--output",
            "after-preview.png",
        ],
    );
    success(
        binary,
        &moved,
        &[
            "project",
            "preview",
            "work.pic",
            "--width",
            "96",
            "--output",
            "small-preview.png",
        ],
    );
    success(
        binary,
        &moved,
        &["project", "export", "work.pic", "--output", "final.png"],
    );
    let final_pixels = pixels(&moved.join("final.png"));
    assert_eq!(final_pixels.dimensions(), (192, 128));
    assert_eq!(
        pixels(&moved.join("small-preview.png")).dimensions(),
        (96, 64)
    );
    assert_eq!(final_pixels, pixels(&dir.join("after-direct.png")));
    assert_eq!(final_pixels, pixels(&moved.join("after-preview.png")));
    assert_ne!(final_pixels, pixels(&dir.join("before-direct.png")));
    assert_eq!(final_pixels.get_pixel(28, 36).0, [0, 0, 0, 0]);
    assert_eq!(final_pixels.get_pixel(36, 36).0, [255, 0, 0, 128]);
    assert_eq!(final_pixels.get_pixel(40, 36).0, [255, 0, 0, 255]);
    assert_eq!(final_pixels.get_pixel(191, 127).0, [0, 0, 0, 0]);
    let text_pixels = (64..120)
        .flat_map(|y| (56..184).map(move |x| (x, y)))
        .filter(|&(x, y)| final_pixels.get_pixel(x, y)[3] > 0)
        .count();
    assert!(text_pixels > 250);
    let before_manifest = fs::read(moved.join("work.pic/manifest.json")).unwrap();
    assert_eq!(
        call(
            binary,
            &moved,
            &[
                "project",
                "layer",
                "set",
                "work.pic",
                "--target",
                "caption",
                "--visible",
                "false",
                "--expect-revision",
                "r9"
            ]
        )["error"]["code"],
        "revision_conflict"
    );
    assert_eq!(
        fs::read(moved.join("work.pic/manifest.json")).unwrap(),
        before_manifest
    );
    assert!(
        success(binary, &moved, &["project", "checkpoint", "work.pic"])["disk_stored"]
            .as_bool()
            .unwrap()
    );
    let cached = success(binary, &moved, &["project", "inspect", "work.pic"]);
    assert_eq!(cached["operations_replayed"], 0);
    let asset = moved
        .join("work.pic/assets")
        .join(font.strip_prefix("asset:").unwrap());
    let bytes = fs::read(&asset).unwrap();
    fs::remove_file(&asset).unwrap();
    assert_eq!(
        call(
            binary,
            &moved,
            &["project", "preview", "work.pic", "--output", "missing.png"]
        )["error"]["code"],
        "asset_missing"
    );
    assert!(!moved.join("missing.png").exists());
    fs::write(asset, bytes).unwrap();
    success(binary, &moved, &["project", "cache-clear", "work.pic"]);
    let replay = success(
        binary,
        &moved,
        &["project", "export", "work.pic", "--output", "replayed.png"],
    );
    assert_eq!(replay["operations_replayed"], 10);
    assert_eq!(final_pixels, pixels(&moved.join("replayed.png")));
    let state = success(binary, &moved, &["project", "inspect", "work.pic"]);
    assert_eq!(state["document"], cached["document"]);
    assert_eq!(state["commits"].as_array().unwrap().len(), 2);
    assert_eq!(
        call(
            binary,
            &moved,
            &[
                "project",
                "template-export",
                "work.pic",
                "--output",
                "unsupported.json"
            ]
        )["error"]["code"],
        "unsupported_template"
    );
    let evidence = json!({"canvas":[192,128],"before_revision":"r9","after_revision":"r10","commits":2,"logical_steps":10,
        "font":font,"text_covered_pixels":text_pixels,"masked_pixels":{"zero":[0,0,0,0],"half":[255,0,0,128],"full":[255,0,0,255]},
        "direct_preview_export_equal":true,"undo_redo_equal":true,"checkpoint_metadata_equal":true,"cold_replay_equal":true,
        "moved_project_and_removed_imports":true,"missing_embedded_font_rejected":true,"stale_revision_rejected":true,"wall_ms":started.elapsed().as_secs_f64()*1000.0});
    fs::write(
        dir.join("evidence.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
    evidence
}

pub fn flat_4k(binary: &Path, dir: &Path) -> Value {
    let started = Instant::now();
    RgbaImage::from_pixel(3840, 2160, Rgba([0, 0, 255, 255]))
        .save(dir.join("4k-back.png"))
        .unwrap();
    RgbaImage::from_pixel(3840, 2160, Rgba([255, 0, 0, 128]))
        .save(dir.join("4k-front.png"))
        .unwrap();
    write_pipeline(
        dir,
        "4k.json",
        vec![op(
            "layer_add",
            "canvas",
            json!({"id":"front","name":"Front","source":"4k-front.png"}),
        )],
    );
    let result = success(
        binary,
        dir,
        &[
            "run",
            "--input",
            "4k-back.png",
            "--pipeline",
            "4k.json",
            "--output",
            "4k.png",
        ],
    );
    let output = pixels(&dir.join("4k.png"));
    assert_eq!(output.dimensions(), (3840, 2160));
    assert_eq!(output.get_pixel(0, 0).0, [188, 0, 187, 255]);
    assert_eq!(output.get_pixel(3839, 2159).0, [188, 0, 187, 255]);
    // Exercise project replay too, using default buffer admission and no derived snapshot.
    success(
        binary,
        dir,
        &[
            "project",
            "create",
            "--input",
            "4k-back.png",
            "--output",
            "4k.pic",
        ],
    );
    success(
        binary,
        dir,
        &[
            "project",
            "apply",
            "4k.pic",
            "--pipeline",
            "4k.json",
            "--expect-revision",
            "r0",
        ],
    );
    success(
        binary,
        dir,
        &["project", "export", "4k.pic", "--output", "4k-project.png"],
    );
    assert_eq!(output, pixels(&dir.join("4k-project.png")));
    json!({"canvas":[3840,2160],"layers":2,"default_limits":true,"admitted_pixel_bytes":3840_u64*2160*80,
        "run_project_pixels_equal":true,"corner_rgba":[188,0,187,255],"result":result,"wall_ms":started.elapsed().as_secs_f64()*1000.0})
}
