"""Check the data-only exporter, including after portable archive relocation."""

import hashlib
import json
import subprocess
import sys
from pathlib import Path

# Canonical semantic payload of numerical.va, qualified independently against
# analytic F/Q and local Jacobians. Producer/source provenance varies by build
# and line endings; the numerical program must agree bit-for-bit on every host.
EXPECTED_SEMANTIC_SHA256 = (
    "700cf1011c4f99ea6d1fb49efa43c2be27db78e5e96bf21ce1769fda0fbb1955"
)


def check(binary, output):
    binary, output = Path(binary).resolve(), Path(output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    source = Path(__file__).with_name("numerical.va")
    artifact = output / "numerical.json"
    subprocess.run([str(binary), str(source), str(artifact)], check=True, timeout=30)
    first = artifact.read_bytes()
    subprocess.run([str(binary), str(source), str(artifact)], check=True, timeout=30)
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
    version = subprocess.check_output(
        [str(binary), "--version"], text=True, timeout=30
    ).strip()
    assert f"revision={data['producer']['revision']}" in version
    semantics = {key: value for key, value in data.items() if key != "producer"}
    semantic_sha256 = hashlib.sha256(
        json.dumps(semantics, sort_keys=True, separators=(",", ":")).encode("utf-8")
    ).hexdigest()
    if semantic_sha256 != EXPECTED_SEMANTIC_SHA256:
        raise RuntimeError(
            f"Numerical program differs from qualified fixture: {semantic_sha256}"
        )

    # Exercise actual filesystem/CLI Unicode handling, including MSYS2 and the
    # standalone Windows executable. Paths must never affect program contents.
    relocated = output / "model with spaces 空间.va"
    relocated.write_bytes(source.read_bytes())
    other_artifact = output / "relocated.json"
    subprocess.run(
        [str(binary), str(relocated), str(other_artifact)], check=True, timeout=30
    )
    assert other_artifact.read_bytes() == first

    unsupported = output / "unsupported.va"
    unsupported.write_text(
        '`include "disciplines.vams"\n'
        "module unsupported(p,n); inout p,n; electrical p,n;\n"
        "parameter integer count=2; analog I(p,n)<+count*V(p,n); endmodule\n",
        encoding="utf-8",
    )
    rejected = output / "unsupported.json"
    result = subprocess.run(
        [str(binary), str(unsupported), str(rejected)],
        capture_output=True,
        check=False,
        timeout=30,
    )
    assert result.returncode != 0 and not rejected.exists()
    assert b"unsupported parameter layout" in result.stderr
    return {
        "schema": data["schema"],
        "version": 1,
        "sha256": hashlib.sha256(first).hexdigest(),
        "semantic_sha256": semantic_sha256,
        "deterministic": True,
        "relocated_source_spaces_unicode": True,
        "unsupported_rejected": True,
    }


if __name__ == "__main__":
    print(json.dumps(check(sys.argv[1], sys.argv[2])))
