"""Verify the exact archive after relocation with an empty tool search path."""

import hashlib
import json
import os
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path


def verify(archive, hide_environment=None):
    archive = Path(archive).resolve()
    output = archive.parent
    hidden = None
    try:
        if hide_environment:
            environment = Path(hide_environment).resolve()
            hidden = environment.with_name(
                environment.name + "-hidden-for-portable-test"
            )
            if hidden.exists():
                raise RuntimeError(f"Refusing to overwrite {hidden}")
            environment.rename(hidden)
        with tempfile.TemporaryDirectory(prefix="OpenVAF portable 空间 ") as temporary:
            root = Path(temporary)
            with zipfile.ZipFile(archive) as source:
                for item in source.infolist():
                    if not (root / item.filename).resolve().is_relative_to(root):
                        raise RuntimeError(f"Unsafe archive path: {item.filename}")
                source.extractall(root)
                for item in source.infolist():
                    path = root / item.filename
                    if path.is_file():
                        path.chmod((item.external_attr >> 16) & 0o777)
            stage = next(path for path in root.iterdir() if path.is_dir())
            empty = root / "empty-path"
            empty.mkdir()
            env = {
                key: value
                for key, value in os.environ.items()
                if key.upper() in {"SYSTEMROOT", "WINDIR", "TEMP", "TMP", "HOME"}
            }
            env["PATH"] = str(empty)
            exe = "openvaf-r.exe" if os.name == "nt" else "openvaf-r"
            subprocess.run(
                [
                    sys.executable,
                    str(stage / "check_metadata.py"),
                    str(stage / "bin" / exe),
                    str(root / "results"),
                ],
                env=env,
                check=True,
            )
            receipt = json.loads((root / "results/metadata-check.json").read_text())
            for name in ("affine.osdi", "nonlinear.osdi"):
                if not (root / "results" / name).is_file():
                    raise RuntimeError(f"Missing portable test artifact: {name}")
        with archive.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        record = {
            "archive": archive.name,
            "sha256": digest,
            "empty_path": True,
            "environment_hidden": bool(hide_environment),
            "relocated_spaces_unicode": True,
            "metadata": receipt,
        }
        destination = output / (archive.stem + "-portable-check.json")
        destination.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(record, indent=2))
    finally:
        if hidden:
            hidden.rename(environment)


if __name__ == "__main__":
    verify(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else None)
