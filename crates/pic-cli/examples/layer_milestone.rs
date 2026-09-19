//! cargo run --release -p pic-cli --example layer_milestone -- BIN OUTPUT_DIR [--4k]
#[path = "support/layer_milestone.rs"]
mod milestone;
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    assert!(
        (2..=3).contains(&args.len()),
        "usage: layer_milestone BIN NEW_OUTPUT_DIR [--4k]"
    );
    let binary = std::fs::canonicalize(&args[0]).unwrap();
    let dir = std::path::PathBuf::from(&args[1]);
    std::fs::create_dir(&dir).unwrap();
    let dir = std::fs::canonicalize(dir).unwrap();
    let mut report = serde_json::json!({"milestone":milestone::run(&binary,&dir)});
    if args.len() == 3 {
        assert_eq!(args[2], "--4k");
        report["flat_4k"] = milestone::flat_4k(&binary, &dir);
    }
    std::fs::write(
        dir.join("report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
