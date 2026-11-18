"""Build the compiler inside the locked Pixi LLVM environment."""

import os
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parents[2]
prefix = subprocess.check_output(["llvm-config", "--prefix"], text=True).strip()
env = dict(os.environ, LLVM_SYS_211_PREFIX=prefix)
subprocess.run(
    [
        "cargo",
        "+1.94.0",
        "build",
        "--locked",
        "--release",
        "-p",
        "openvaf-driver",
        "--features",
        "llvm21",
    ],
    cwd=root,
    env=env,
    check=True,
)
