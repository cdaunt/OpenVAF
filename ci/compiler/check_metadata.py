"""Compile positive/negative OSDI 0.5 constness regression fixtures."""

import ctypes as c
import json
import subprocess
import sys
from pathlib import Path


class Entry(c.Structure):
    _fields_ = [
        ("node_1", c.c_uint32),
        ("node_2", c.c_uint32),
        ("react_ptr_off", c.c_uint32),
        ("flags", c.c_uint32),
    ]


class DescriptorPrefix(c.Structure):
    # Prefix of the repository's openvaf/osdi/header/osdi_0_5.h.
    _fields_ = [
        ("name", c.c_char_p),
        ("num_nodes", c.c_uint32),
        ("num_terminals", c.c_uint32),
        ("nodes", c.c_void_p),
        ("num_jacobian_entries", c.c_uint32),
        ("jacobian_entries", c.POINTER(Entry)),
    ]


def inspect(path, affine):
    library = c.CDLL(str(path.resolve()))
    version = tuple(
        c.c_uint32.in_dll(library, name).value
        for name in ("OSDI_VERSION_MAJOR", "OSDI_VERSION_MINOR")
    )
    assert version == (0, 5), f"Expected OSDI 0.5, got {version}"
    assert c.c_uint32.in_dll(library, "OSDI_NUM_DESCRIPTORS").value == 1
    descriptor = DescriptorPrefix.in_dll(library, "OSDI_DESCRIPTORS")
    flags = [
        descriptor.jacobian_entries[i].flags
        for i in range(descriptor.num_jacobian_entries)
    ]
    assert flags, "Fixture emitted no Jacobian entries"
    for present, constant in ((4, 1), (8, 2)):
        active = [flag for flag in flags if flag & present]
        assert active, f"Fixture missing Jacobian part {present}"
        if affine:
            assert all(flag & constant for flag in active), flags
        else:
            assert any(not flag & constant for flag in active), flags
    return {"module": descriptor.name.decode(), "version": version, "flags": flags}


def main():
    compiler = str(Path(sys.argv[1]).resolve())
    output = Path(sys.argv[2]).resolve()
    output.mkdir(parents=True, exist_ok=True)
    receipt = []
    for name, affine in (("affine", True), ("nonlinear", False)):
        artifact = output / f"{name}.osdi"
        subprocess.run(
            [
                compiler,
                "--target_cpu",
                "generic",
                str(Path(__file__).with_name(f"{name}.va").resolve()),
                "-o",
                str(artifact),
            ],
            check=True,
        )
        receipt.append(inspect(artifact, affine))
    (output / "metadata-check.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt, indent=2))


if __name__ == "__main__":
    main()
