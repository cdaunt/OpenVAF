"""Build the compiler inside the locked Pixi LLVM environment."""

import os
import shutil
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parents[2]
prefix = subprocess.check_output(["llvm-config", "--prefix"], text=True).strip()
env = dict(os.environ, LLVM_SYS_211_PREFIX=prefix)
if os.name == "nt":
    # LLVM's exported CMake system-library names differ from conda-forge's
    # import-library filenames. Supply build-local aliases for the same DLLs.
    library = Path(prefix) / "lib"
    aliases = {
        "z.lib": ("zlib.lib", "zdll.lib"),
        "zstd.dll.lib": ("zstd.lib",),
        "xml2.lib": ("libxml2.lib",),
    }
    aliases_dir = root / "build/llvm-system-libs"
    aliases_dir.mkdir(parents=True, exist_ok=True)
    for expected, candidates in aliases.items():
        source = next(
            (
                library / name
                for name in (expected, *candidates)
                if (library / name).is_file()
            ),
            None,
        )
        if source is None:
            raise RuntimeError(
                f"Missing LLVM import library {expected}; checked {candidates}"
            )
        shutil.copy2(source, aliases_dir / expected)

command = [
    "cargo",
    "+1.94.0",
    "rustc" if os.name == "nt" else "build",
    "--locked",
    "--release",
    "-p",
    "openvaf-driver",
    "--features",
    "llvm21",
]
if os.name == "nt":
    command.extend(["--", "-L", f"native={aliases_dir}"])
subprocess.run(command, cwd=root, env=env, check=True)
