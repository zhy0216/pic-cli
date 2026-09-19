#!/usr/bin/env python3
"""Build a host Linux acceptance bundle, extract it, and verify the shipped examples."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[1]
COPY_TREES = ("docs", "examples", "tests/fonts", "plans/fast-image-editing/roadmap")


def capture(args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "target/dist",
                        help="parent for a new retained run directory (default target/dist)")
    args = parser.parse_args()
    output = args.output.resolve()
    for folder in COPY_TREES:
        source = (ROOT / folder).resolve()
        if output.is_relative_to(source):
            parser.error(f"--output must be outside copied source tree: {source}")
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("this packaging route has only been validated on Linux x86_64")
    rustc = capture(["rustc", "-vV"])
    host = next(line.removeprefix("host: ") for line in rustc.splitlines() if line.startswith("host: "))
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    output.mkdir(parents=True, exist_ok=True)
    run = Path(tempfile.mkdtemp(prefix="pic-cli-", dir=output))
    print(f"Build and verification artifacts: {run}", file=sys.stderr, flush=True)
    name = f"pic-cli-{version}-{host}"
    package = run / name
    package.mkdir()
    build_command = ["cargo", "build", "--locked", "--release", "-p", "pic-cli", "--bin", "pic-cli",
                     "--example", "agent_fixtures", "--example", "layer_milestone",
                     "--target", host, "--target-dir", str(ROOT / "target")]
    # Pin the host explicitly even when the caller has CARGO_BUILD_TARGET set.
    with (run / "build.log").open("w") as log:
        subprocess.run(build_command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True)
    release = ROOT / "target" / host / "release"
    (package / "bin").mkdir()
    (package / "libexec").mkdir()
    shutil.copy2(release / "pic-cli", package / "bin/pic-cli")
    for example in ("agent_fixtures", "layer_milestone"):
        shutil.copy2(release / "examples" / example, package / "libexec" / example)
    for folder in COPY_TREES:
        shutil.copytree(ROOT / folder, package / folder)
    for file in ("README.md", "Cargo.lock", "Cargo.toml", "scripts/verify-package.py"):
        (package / file).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / file, package / file)
    metadata = json.loads(capture(["cargo", "metadata", "--locked", "--format-version", "1"]))
    dependencies = [{key: p[key] for key in ("name", "version", "license", "repository")}
                    for p in metadata["packages"] if p["source"] is not None]
    (package / "DEPENDENCIES.json").write_text(json.dumps(dependencies, indent=2) + "\n")
    linkage = capture(["ldd", str(package / "bin/pic-cli")])
    (package / "runtime-libraries.txt").write_text(linkage + "\n")
    build = {"version": version, "host": host, "rustc": rustc, "cargo": capture(["cargo", "--version"]),
             "built_at_utc": datetime.now(timezone.utc).isoformat(),
             "source_commit": capture(["git", "rev-parse", "HEAD"]),
             "source_dirty": bool(capture(["git", "status", "--porcelain"])),
             "cargo_lock_sha256": digest(ROOT / "Cargo.lock"), "build_command": build_command,
             "platform": platform.platform(), "libc": platform.libc_ver(),
             "build_environment": {key: os.environ[key] for key in
                 ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_PROFILE_RELEASE_LTO",
                  "CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "CARGO_BUILD_JOBS") if key in os.environ}}
    (package / "build-info.json").write_text(json.dumps(build, indent=2) + "\n")
    sums = "".join(f"{digest(p)}  {p.relative_to(package)}\n"
                   for p in sorted(package.rglob("*")) if p.is_file())
    (package / "SHA256SUMS").write_text(sums)
    archive = run / (name + ".tar.gz")
    with tarfile.open(archive, "w:gz") as bundle:
        bundle.add(package, arcname=name)
    (run / (archive.name + ".sha256")).write_text(f"{digest(archive)}  {archive.name}\n")
    # Verify the actual archive, from an independent temporary cwd, never the staging binary.
    with tempfile.TemporaryDirectory(prefix="pic-package-extract-") as tmp:
        with tarfile.open(archive, "r:gz") as bundle:
            bundle.extractall(tmp, filter="data")
        extracted = Path(tmp) / name
        subprocess.run([sys.executable, str(extracted / "scripts/verify-package.py"), str(extracted),
                        "--output", str(run / "verification"), "--4k"], cwd=tmp, check=True)
    result = {"ok": True, "archive": str(archive), "binary": str(package / "bin/pic-cli"),
              "verification": str(run / "verification/verification.json"),
              "archive_sha256": digest(archive)}
    (run / "package-result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
