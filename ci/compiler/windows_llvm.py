"""Download the checksum-pinned MSYS2 LLVM 21 packages used only by CI."""

import hashlib
import json
import sys
from pathlib import Path
from urllib.request import urlopen


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def download(output, entries):
    output.mkdir(parents=True, exist_ok=True)
    paths = []
    for entry in entries:
        path = output / entry["url"].rsplit("/", 1)[1]
        if not path.exists():
            temporary = path.with_suffix(".download")
            try:
                with (
                    urlopen(entry["url"], timeout=60) as source,
                    temporary.open("wb") as target,
                ):
                    while chunk := source.read(1024 * 1024):
                        target.write(chunk)
                if digest(temporary) != entry["sha256"]:
                    raise ValueError(f"LLVM package checksum mismatch: {path.name}")
                temporary.replace(path)
            finally:
                temporary.unlink(missing_ok=True)
        if digest(path) != entry["sha256"]:
            raise ValueError(f"LLVM package checksum mismatch: {path.name}")
        paths.append(path)
    return paths


def main():
    entries = json.loads(Path(__file__).with_name("windows-llvm.json").read_text())
    for path in download(Path(sys.argv[1]), entries):
        print(path)


if __name__ == "__main__":
    main()
