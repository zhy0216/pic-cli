#!/usr/bin/env python3
"""Run the shipped guide and failure recovery against a packaged binary, outside the repo."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import traceback


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def envelope(raw, version, ok):
    require(len(raw.splitlines()) == 1, "stdout must contain exactly one JSON line")
    value = json.loads(raw)
    require(value["schema_version"] == 1, "unexpected result schema")
    require(value["engine_version"] == version, "engine/build version mismatch")
    require(value["ok"] is ok, "unexpected ok field")
    require((value["error"] is None) is ok, "unexpected error field")
    require((value["data"] is not None) is ok, "unexpected data field")
    require(isinstance(value["warnings"], list), "warnings must be an array")
    for stage in ("validation", "read", "decode", "process", "encode", "write", "total"):
        ms = value["timings"][stage + "_ms"]
        require(math.isfinite(ms) and ms >= 0, "invalid timing")
    return value


def verify(package, cwd, include_4k):
    for line in (package / "SHA256SUMS").read_text().splitlines():
        expected, name = line.split("  ", 1)
        require(digest(package / name) == expected, f"package checksum mismatch: {name}")
    build = json.loads((package / "build-info.json").read_text())
    version = build["version"]
    binary = package / "bin/pic-cli"
    fixture = package / "libexec/agent_fixtures"
    log = cwd / "calls.jsonl"

    def call(args, code=0, error=None, json_mode=True):
        argv = [str(binary), *(["--json"] if json_mode else []), *args]
        output = subprocess.run(argv, cwd=cwd, capture_output=True, text=True, check=False)
        with log.open("a") as stream:
            stream.write(json.dumps({"argv": argv, "exit_code": output.returncode,
                                    "stdout": output.stdout, "stderr": output.stderr}) + "\n")
        require(output.returncode == code, f"{args}: {output}")
        if not json_mode:
            return output
        require(not output.stderr, f"unexpected CLI stderr: {output.stderr}")
        value = envelope(output.stdout, version, code == 0)
        if error:
            require(value["error"]["code"] == error, f"{args}: {value}")
        return value["data"] if code == 0 else value

    guide = (package / "docs/agent-guide.md").read_text()
    blocks = re.findall(r"```sh\n(# pic-verify: ([^\n]+)\n.*?)\n```", guide, re.S)
    require([name for _, name in blocks] == ["setup", "ordinary", "project", "history-template", "layers"],
            "agent guide executable blocks missing or reordered")
    script = "\n".join(block for block, _ in blocks) + "\n"
    (cwd / "guide.sh").write_text(script)
    env = dict(os.environ, PIC_PACKAGE_ROOT=str(package))
    with (cwd / "guide.stdout").open("w") as out, (cwd / "guide.stderr").open("w") as err:
        result = subprocess.run(["sh", "-eux", "guide.sh"], cwd=cwd, env=env,
                                stdout=out, stderr=err, check=False)
    require(result.returncode == 0, "guide failed; see guide.stderr and receipts/")
    receipts = {}
    for path in sorted((cwd / "receipts").glob("*.json")):
        receipts[path.stem] = envelope(path.read_text(), version, True)["data"]
    require(receipts["version"]["version"] == version, "version command mismatch")
    # Exercise the documented copy-install route, with a disposable personal prefix.
    installed = cwd / "installed/bin/pic-cli"
    installed.parent.mkdir(parents=True)
    subprocess.run(["install", "-m", "755", str(binary), str(installed)], cwd=cwd, check=True)
    result = subprocess.run([str(installed), "--json", "--version"], cwd=installed.parent,
                            capture_output=True, text=True, check=True)
    require(not result.stderr, "installed binary emitted unexpected stderr")
    require(envelope(result.stdout, version, True)["data"]["version"] == version,
            "installed binary version mismatch")
    require(digest(installed) == digest(binary), "installed binary changed")
    capabilities = receipts["capabilities"]
    require(capabilities["result_schema_version"] == 1 and
            capabilities["pipeline_schema_version"] == 1, "capabilities version mismatch")
    for operation in capabilities["operations"]:
        require(operation["op_version"] == 1, "unsupported operation version in guide")
    smart = next(c for c in capabilities["capabilities"] if c["id"] == "smart_editing")
    require(smart["status"] == "not_implemented" and smart["scope"] == "roadmap",
            "smart feature status must be truthful")
    # Every discoverable command must have working JSON help, including nested commands.
    for command in capabilities["commands"]:
        args = [] if command == "help" else command.split()
        call([*args, "--help"])
    require(call(["--version"], json_mode=False).stdout.strip() == f"pic-cli {version}",
            "text version mismatch")
    require("Usage:" in call(["--help"], json_mode=False).stdout, "text help missing")
    preview = receipts["preview"]
    mapping = preview["coordinates"]["preview_to_canvas"]
    require(mapping == {"scale": [2.0, 2.0], "offset": [8.0, 6.0]}, "preview mapping mismatch")
    point = [mapping["scale"][i] * p + mapping["offset"][i] for i, p in enumerate([3.5, 4.5])]
    require(point == [15, 15], "preview pixel center mapped incorrectly")
    require(preview["preview_size"] == {"width": 32, "height": 16}, "preview size mismatch")
    require(receipts["inspect"]["replay"]["reused_steps"] == 2, "checkpoint was not used")
    require(receipts["old-inspect"]["current_revision"] == receipts["redo"]["revision"],
            "old inspection moved the pointer")
    require(receipts["branchless-continue"]["base_revision"] == "r1", "wrong continuation base")
    require(receipts["replay-export"]["operations_replayed"] == 2, "P0 cold replay missing")
    require(receipts["layers-replay"]["operations_replayed"] == 11, "layer cold replay missing")

    comparisons = []

    def compare(left, right):
        result = subprocess.run([str(fixture), "compare", left, right], cwd=cwd,
                                capture_output=True, text=True, check=True)
        comparisons.append({"left": left, "right": right, **json.loads(result.stdout)})

    for pair in [("direct.png", "before-followup.png"), ("direct.png", "undone.png"),
                 ("final.png", "redone.png"), ("final.png", "preserved-old.png"),
                 ("revised.png", "replayed.png"), ("layers-direct.png", "layers-before.png"),
                 ("layers-final.png", "layers-preview.png"),
                 ("layers-final.png", "layers-replayed.png")]:
        compare(*pair)
    template = json.loads((cwd / "recipe.json").read_text())
    bindings = json.loads((cwd / "recipes/bindings.json").read_text())
    pipeline = {"schema_version": 1, "operations": [
        {"op": s["op"], "op_version": s["op_version"],
         "target": bindings["targets"][s["target_slot"]],
         "params": bindings["params"][s["params_slot"]]} for s in template["operations"]]}
    (cwd / "rebound.json").write_text(json.dumps(pipeline))
    call(["run", "--input", "new-photo.png", "--pipeline", "rebound.json", "--output", "rebound.png"])
    compare("another.png", "rebound.png")
    old = call(["project", "inspect", "moved/work.pic", "--revision", receipts["apply"]["revision"]])
    require(old["document"] == receipts["inspect"]["document"], "old document changed")
    for name in ["photo.png", "background.png", "subject.png", "mask.png", "font.ttf"]:
        require(not (cwd / name).exists(), f"external import still present: {name}")

    # Errors must not publish an output or alter either authoritative manifest.
    manifests = {p: p.read_bytes() for p in (cwd / "moved").glob("*.pic/manifest.json")}
    existing = (cwd / "replayed.png").read_bytes()
    call(["resize", "--input", "new-photo.png"], 2, "invalid_argument")
    call(["info", "absent.png"], 1, "file_not_found")
    call(["resize", "--input", "new-photo.png", "--output", "failed.png"], 1, "invalid_argument")
    (cwd / "bad.json").write_text("{broken")
    call(["run", "--input", "new-photo.png", "--pipeline", "bad.json", "--output", "failed.png"],
         1, "invalid_json")
    (cwd / "bad.json").write_text(json.dumps({"schema_version": 999, "operations": []}))
    call(["run", "--input", "new-photo.png", "--pipeline", "bad.json", "--output", "failed.png"],
         1, "unsupported_version")
    (cwd / "bad.json").write_text(json.dumps({"schema_version": 1, "operations": [
        {"op": "generative_fill", "op_version": 1, "target": "canvas", "params": {}}]}))
    call(["run", "--input", "new-photo.png", "--pipeline", "bad.json", "--output", "failed.png"],
         1, "unknown_operation")
    call(["identity", "--input", "new-photo.png", "--output", "replayed.png"], 1, "output_exists")
    call(["project", "export", "moved/layers.pic", "--output", "failed.jpg"], 1, "alpha_not_supported")
    call(["project", "undo", "moved/work.pic", "--expect-revision", "r0"], 1, "revision_conflict")
    call(["project", "redo", "moved/work.pic", "--expect-revision", receipts["revise"]["revision"]],
         1, "history_boundary")
    call(["project", "template-export", "moved/layers.pic", "--output", "unsupported.json"],
         1, "unsupported_template")
    font = next(x for x in receipts["layers-inspect"]["document"]["layers"] if x["id"] == "title")["kind"]["params"]["font"]
    text_args = ["project", "text", "set", "moved/layers.pic", "--target", "title", "--width", "128",
                 "--height", "64", "--expect-revision", receipts["text-set"]["revision"]]
    call([*text_args, "--text", "Text", "--font", "absent.ttf"], 1, "file_not_found")
    call([*text_args, "--text", "中", "--font", font], 1, "missing_glyph")
    incomplete = dict(bindings, params={})
    (cwd / "incomplete.json").write_text(json.dumps(incomplete))
    call(["project", "template-run", "--template", "recipe.json", "--bindings", "incomplete.json",
          "--output", "failed.pic"], 1, "invalid_argument")
    call(["project", "checkpoint", "moved/layers.pic"])
    asset = cwd / "moved/layers.pic/assets" / font.removeprefix("asset:")
    data = asset.read_bytes()
    try:
        asset.unlink()
        call(["project", "preview", "moved/layers.pic", "--output", "failed.png"], 1, "asset_missing")
        asset.write_bytes(b"corrupt font")
        call(["project", "preview", "moved/layers.pic", "--output", "failed.png"], 1, "integrity_mismatch")
    finally:
        asset.write_bytes(data)
    call(["project", "export", "moved/layers.pic", "--output", "recovered.png"])
    compare("layers-final.png", "recovered.png")
    plain = call(["info", "absent.png"], 1, json_mode=False)
    require(plain.stderr and json.loads(plain.stdout)["error"]["code"] == "file_not_found",
            "non-JSON errors must also explain the failure on stderr")
    for path, data in manifests.items():
        require(path.read_bytes() == data, "failed operation changed manifest")
    require((cwd / "replayed.png").read_bytes() == existing, "failed overwrite changed output")
    for name in ["failed.png", "failed.jpg", "failed.pic", "unsupported.json"]:
        require(not (cwd / name).exists(), f"failed request published {name}")

    args = [str(package / "libexec/layer_milestone"), str(binary), str(cwd / "layer-milestone")]
    if include_4k:
        args.append("--4k")
    with (cwd / "milestone.stdout").open("w") as out, (cwd / "milestone.stderr").open("w") as err:
        subprocess.run(args, cwd=cwd, stdout=out, stderr=err, check=True)
    return {"ok": True, "version": version, "binary_sha256": digest(binary),
            "copy_install_verified": True,
            "guide_blocks": [name for _, name in blocks], "guide_receipts": len(receipts),
            "discovered_commands_checked": len(capabilities["commands"]),
            "calls_checked": len(log.read_text().splitlines()), "pixel_comparisons": comparisons,
            "external_imports_removed": True, "failed_requests_preserved_authority": True,
            "milestone": json.loads((cwd / "layer-milestone/report.json").read_text())}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path, help="extracted package root")
    parser.add_argument("--output", type=Path, required=True, help="new directory retaining all evidence")
    parser.add_argument("--4k", dest="include_4k", action="store_true", help="also check ordinary 4K two-layer output")
    args = parser.parse_args()
    package, output = args.package.resolve(), args.output.resolve()
    require(not output.exists(), f"output must not exist: {output}")
    output.parent.mkdir(parents=True, exist_ok=True)
    cwd = Path(tempfile.mkdtemp(prefix="pic-package-verify-"))
    report = {"ok": False, "execution_cwd": str(cwd), "retained_evidence": str(output)}
    try:
        report.update(verify(package, cwd, args.include_4k))
    except Exception:
        report["failure"] = traceback.format_exc()
        raise
    finally:
        (cwd / "verification.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.move(str(cwd), output)
        print(f"Verification evidence: {output / 'verification.json'}")


if __name__ == "__main__":
    main()
