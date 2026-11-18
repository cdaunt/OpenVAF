"""Bundle native runtime dependencies from the locked Pixi environment."""

import json
import os
import platform
import re
import shutil
import subprocess
from pathlib import Path

SYSTEM_ELF = {
    "libc.so.6",
    "libm.so.6",
    "libpthread.so.0",
    "librt.so.1",
    "libdl.so.2",
    "libutil.so.1",
    "libresolv.so.2",
}


def capture(*args, env=None):
    return subprocess.check_output(args, text=True, env=env).strip()


def dependencies(image, prefix):
    system = platform.system()
    if system == "Linux":
        env = dict(os.environ, LD_LIBRARY_PATH=str(prefix / "lib"))
        result = capture("ldd", str(image), env=env)
        if "not found" in result:
            raise RuntimeError(result)
        return [
            (name, Path(path))
            for name, path in re.findall(r"^\s*(\S+) => (/\S+)", result, re.MULTILINE)
            if name not in SYSTEM_ELF
        ]
    if system == "Darwin":
        identities = set(capture("otool", "-D", str(image)).splitlines()[1:])
        names = [
            line.strip().split(" (", 1)[0]
            for line in capture("otool", "-L", str(image)).splitlines()[1:]
        ]
        result = []
        for name in names:
            if name in identities or name.startswith(("/usr/lib/", "/System/")):
                continue
            # All non-system dylibs must come from this locked environment.
            path = prefix / "lib" / Path(name).name
            if not path.is_file():
                raise RuntimeError(f"Unresolved dylib {name} for {image}")
            if path.resolve() != image.resolve():
                result.append((name, path))
        return result
    text = capture("llvm-objdump", "-p", str(image))
    result = []
    for name in re.findall(r"DLL Name:\s*(\S+)", text):
        candidates = [
            prefix / "Library/bin" / name,
            prefix / "bin" / name,
            prefix / name,
        ]
        path = next((path for path in candidates if path.is_file()), None)
        if path:
            result.append((name, path))
        elif name.lower().startswith(("api-ms-", "ext-ms-")):
            continue
        elif not (Path(os.environ["SYSTEMROOT"]) / "System32" / name).is_file():
            raise RuntimeError(f"Unresolved DLL {name} for {image}")
    return result


def copy_notices(prefix, sources, output):
    """Retain package identities, licenses and available build/source recipes."""
    records = []
    remaining = {path.resolve() for path in sources}
    for metadata in sorted((prefix / "conda-meta").glob("*.json")):
        record = json.loads(metadata.read_text(encoding="utf-8"))
        files = {(prefix / name).resolve() for name in record.get("files", [])}
        if not remaining.intersection(files):
            continue
        remaining.difference_update(files)
        destination = output / metadata.stem
        destination.mkdir(parents=True)
        shutil.copy2(metadata, destination / "package.json")
        cache = Path(record["link"]["source"]) / "info"
        licenses = cache / "licenses"
        if licenses.is_dir():
            shutil.copytree(licenses, destination / "licenses")
        else:
            installed = [
                name for name in record.get("files", []) if "/licenses/" in name.lower()
            ]
            if not installed:
                raise RuntimeError(f"Missing package license texts: {record['name']}")
            for name in installed:
                target = destination / "licenses" / name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(prefix / name, target)
            if record["license"].startswith("GPL-3.0"):
                shutil.copy2(
                    Path(__file__).resolve().parents[2] / "LICENSE",
                    destination / "licenses/GPL-3.0.txt",
                )
        for name in ("recipe", "about.json", "git"):
            source = cache / name
            if source.is_dir():
                shutil.copytree(source, destination / name)
            elif source.is_file():
                shutil.copy2(source, destination / name)
        records.append(
            {
                key: record.get(key)
                for key in ("name", "version", "build", "url", "sha256", "license")
            }
        )
    if remaining:
        raise RuntimeError(f"Bundled files have no locked package owner: {remaining}")
    (output / "packages.json").write_text(
        json.dumps(records, indent=2) + "\n", encoding="utf-8"
    )


def windows_import_library(stage):
    """Generate an import library for the operating system's UCRT, not an SDK copy."""
    dll = Path(os.environ["SYSTEMROOT"]) / "System32/ucrtbase.dll"
    exports = capture("llvm-readobj", "--coff-exports", str(dll))
    names = re.findall(r"^\s*Name: (\S+)", exports, re.MULTILINE)
    if not names:
        raise RuntimeError("No UCRT exports found")
    definition = stage / "lib/ucrtbase.def"
    definition.write_text("LIBRARY ucrtbase.dll\nEXPORTS\n" + "\n".join(names) + "\n")
    subprocess.run(
        [
            "llvm-dlltool",
            "-m",
            "i386:x86-64",
            "-d",
            str(definition),
            "-l",
            str(stage / "lib/ucrtbase.lib"),
        ],
        check=True,
    )


def bundle(stage, prefix):
    system = platform.system()
    tool = {"Linux": "ld.lld", "Darwin": "ld64.lld", "Windows": "lld-link.exe"}[system]
    source = Path(shutil.which(tool) or "")
    if not source.is_file():
        raise RuntimeError(f"Missing bundled linker {tool}")
    shutil.copy2(source, stage / "bin" / tool)
    (stage / "lib").mkdir()
    (stage / "bin/openvaf-portable").write_text("native-osdi-0.5\n")
    queue = (
        list((stage / "bin").glob("*.exe"))
        if system == "Windows"
        else [stage / "bin/openvaf-r", stage / "bin" / tool]
    )
    bundled = {}
    sources = {source}
    report = []
    while queue:
        image = queue.pop()
        for old_name, dependency in dependencies(image, prefix):
            dependency = dependency.resolve()
            if dependency.is_relative_to(stage):
                dependency = bundled[dependency.name]
            if not dependency.is_relative_to(prefix):
                raise RuntimeError(
                    f"Dependency outside locked environment: {dependency}"
                )
            name = Path(old_name).name
            destination = stage / ("bin" if system == "Windows" else "lib") / name
            if name in bundled and bundled[name] != dependency:
                raise RuntimeError(f"Conflicting bundled dependency: {name}")
            if name not in bundled:
                bundled[name] = dependency
                sources.add(dependency)
                shutil.copy2(dependency, destination)
                queue.append(destination)
            report.append(
                {
                    "image": str(image.relative_to(stage)),
                    "dependency": old_name,
                    "bundled": str(destination.relative_to(stage)),
                }
            )
            if system == "Darwin":
                relative = "../lib/" if image.parent.name == "bin" else ""
                subprocess.run(
                    [
                        "install_name_tool",
                        "-change",
                        old_name,
                        f"@loader_path/{relative}{name}",
                        str(image),
                    ],
                    check=True,
                )
        if system == "Linux":
            origin = "$ORIGIN/../lib" if image.parent.name == "bin" else "$ORIGIN"
            subprocess.run(["patchelf", "--set-rpath", origin, str(image)], check=True)
        elif system == "Darwin":
            for block in capture("otool", "-l", str(image)).split("Load command "):
                if re.search(r"\bcmd LC_RPATH\b", block):
                    rpath = re.search(r"\bpath (.*?) \(offset", block).group(1)
                    subprocess.run(
                        ["install_name_tool", "-delete_rpath", rpath, str(image)],
                        check=True,
                    )
            if image.parent.name == "lib":
                subprocess.run(
                    ["install_name_tool", "-id", f"@rpath/{image.name}", str(image)],
                    check=True,
                )
    if system == "Darwin":
        for image in list((stage / "lib").iterdir()) + [
            stage / "bin/openvaf-r",
            stage / "bin" / tool,
        ]:
            subprocess.run(
                ["codesign", "--force", "--sign", "-", str(image)], check=True
            )
    if system == "Windows":
        windows_import_library(stage)
    third_party = stage / "third-party"
    third_party.mkdir()
    copy_notices(prefix, sources, third_party)
    (stage / "runtime-dependencies.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
