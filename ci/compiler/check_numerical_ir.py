"""Check the data-only exporter, including after portable archive relocation."""

import hashlib
import json
import subprocess
import sys
from pathlib import Path


def check(binary, output):
    binary, output = Path(binary).resolve(), Path(output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    source = Path(__file__).with_name("numerical.va")
    artifact = output / "numerical.json"
    subprocess.run([str(binary), str(source), str(artifact)], check=True)
    first = artifact.read_bytes()
    subprocess.run([str(binary), str(source), str(artifact)], check=True)
    if artifact.read_bytes() != first:
        raise RuntimeError("nondeterministic numerical IR")
    data = json.loads(first)
    assert data["schema"] == "circuit-numerical-ir" and data["version"] == 1
    assert data["profile"] == "stateless_electrical_fq_v1"
    assert data["variables"] == ["p", "n"]
    assert data["preparation"]["outputs"]
    assert (
        data["producer"]["source_sha256"]
        == hashlib.sha256(source.read_bytes()).hexdigest()
    )
    assert any(
        len(phi["incoming"]) >= 3
        for b in data["evaluation"]["blocks"]
        for phi in b["phis"]
    )
    assert data["jacobian"]
    return {
        "schema": data["schema"],
        "version": 1,
        "sha256": hashlib.sha256(first).hexdigest(),
        "deterministic": True,
    }


if __name__ == "__main__":
    print(json.dumps(check(sys.argv[1], sys.argv[2])))
