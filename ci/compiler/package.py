"""Package a tested candidate compiler and exact corresponding source."""

import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def capture(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def main():
    binary = Path(sys.argv[1]).resolve()
    target = sys.argv[2]
    commit = capture("git", "rev-parse", "HEAD")
    if capture("git", "status", "--porcelain", "--untracked-files=no"):
        raise RuntimeError(
            "Refusing to package tracked changes absent from the source archive"
        )
    output = ROOT / "build/compiler-dist"
    output.mkdir(parents=True, exist_ok=True)
    stem = f"openvaf-r-osdi05-{target}-{commit}"
    stage = ROOT / "build" / stem
    if stage.exists():
        shutil.rmtree(stage)
    (stage / "bin").mkdir(parents=True)
    shutil.copy2(binary, stage / "bin" / binary.name)
    shutil.copy2(ROOT / "LICENSE", stage / "LICENSE")
    shutil.copy2(ROOT / "ci/compiler/README.md", stage / "README.md")
    shutil.copy2(
        ROOT / "build/metadata-check/metadata-check.json", stage / "metadata-check.json"
    )
    for name in ("affine.va", "nonlinear.va", "check_metadata.py"):
        shutil.copy2(ROOT / "ci/compiler" / name, stage / name)
    system = platform.system()
    if system == "Linux":
        deps = capture("ldd", str(binary))
    elif system == "Darwin":
        deps = capture("otool", "-L", str(binary))
    else:
        deps = capture("objdump", "-p", str(binary))
    (stage / "runtime-dependencies.txt").write_text(deps + "\n", encoding="utf-8")
    provenance = {
        "repository": "https://github.com/cdaunt/OpenVAF",
        "commit": commit,
        "source_archive": f"openvaf-r-source-{commit}.zip",
        "target": target,
        "osdi": "0.5",
        "compiler_version": capture(str(binary), "--version"),
        "rust": capture(
            "rustup",
            "run",
            "1.94.0-x86_64-pc-windows-gnu" if system == "Windows" else "1.94.0",
            "rustc",
            "-vV",
        ),
        "host_architecture": platform.machine(),
        "llvm": capture("llvm-config", "--version"),
        "cargo_lock_sha256": hashlib.sha256(
            (ROOT / "Cargo.lock").read_bytes()
        ).hexdigest(),
        "qualification": "affine and nonlinear metadata regressions only; full model-pack qualification is separate",
        "github_run": os.environ.get("GITHUB_RUN_ID"),
    }
    (stage / "provenance.json").write_text(
        json.dumps(provenance, indent=2) + "\n", encoding="utf-8"
    )
    # zipfile preserves executable permission bits from the source file.
    with zipfile.ZipFile(output / f"{stem}.zip", "w", zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(stage.rglob("*")):
            if path.is_file():
                archive.write(path, Path(stem) / path.relative_to(stage))
    subprocess.run(
        [
            "git",
            "archive",
            "--format=zip",
            "--prefix=openvaf-r-source/",
            f"--output={output / provenance['source_archive']}",
            commit,
        ],
        cwd=ROOT,
        check=True,
    )
    # Checksums cover exactly this build's files, excluding stale output.
    paths = [output / f"{stem}.zip", output / provenance["source_archive"]]
    checksums = []
    for path in paths:
        with path.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        checksums.append(f"{digest}  {path.name}\n")
    (output / f"{stem}.sha256").write_text("".join(checksums), encoding="utf-8")
    print(json.dumps(provenance, indent=2))


if __name__ == "__main__":
    main()
