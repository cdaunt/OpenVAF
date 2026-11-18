//! Native linking for the relocatable compiler distribution.
//! Modified in cdaunt/OpenVAF on 2026-11-18; distributed under GPLv3.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use camino::Utf8Path;
use target::spec::Target;

use crate::{Command, Linker};

pub(super) fn directory(use_bundled: bool) -> Result<Option<PathBuf>> {
    if !use_bundled {
        return Ok(None);
    }
    let executable = std::env::current_exe().context("locate portable compiler")?;
    let directory = executable.parent().context("compiler has no parent directory")?;
    Ok(directory.join("openvaf-portable").is_file().then(|| directory.to_owned()))
}

pub(super) fn linker(
    directory: &Path,
    target: &Target,
    output: &Utf8Path,
    add_objects: impl FnOnce(&mut dyn Linker),
) -> Result<Box<dyn Linker>> {
    let native_os = if target.options.is_like_windows {
        "windows"
    } else if target.options.is_like_osx {
        "macos"
    } else {
        "linux"
    };
    if native_os != std::env::consts::OS || target.arch != std::env::consts::ARCH {
        bail!("portable archives support only the native target; use an explicit linker for cross-compilation");
    }
    let name = match native_os {
        "windows" => "lld-link.exe",
        "macos" => "ld64.lld",
        _ => "ld.lld",
    };
    let executable = directory.join(name);
    if !executable.is_file() {
        bail!("portable compiler is incomplete: missing {}", executable.display());
    }
    let mut cmd = Command::new(executable);
    match native_os {
        "windows" => {
            cmd.args(["/DLL", "/NOENTRY", "/NODEFAULTLIB", "/NOIMPLIB", "/MACHINE:X64"]);
            for name in ["ucrtbase.lib", "kernelbase.lib", "runtime.obj"] {
                let path = directory.join("../lib").join(name);
                if !path.is_file() {
                    bail!("portable compiler is incomplete: missing {}", path.display());
                }
                cmd.arg(path);
            }
        }
        "macos" => {
            // The OSDI host supplies libSystem symbols. Dynamic lookup avoids
            // requiring or redistributing an Apple SDK on the consumer machine.
            let arch = match target.arch.as_str() {
                "aarch64" => "arm64",
                "x86_64" => "x86_64",
                other => bail!("unsupported portable macOS architecture: {other}"),
            };
            cmd.args([
                "-dylib",
                "-undefined",
                "dynamic_lookup",
                "-arch",
                arch,
                "-platform_version",
                "macos",
                "13.0",
                "13.0",
            ]);
        }
        _ => {
            cmd.args(["-shared", "-m", "elf_x86_64", "--hash-style=gnu"]);
        }
    }
    let mut linker = PortableLinker { cmd, windows: native_os == "windows" };
    add_objects(&mut linker);
    linker.output_filename(output);
    // Preserve explicit caller flags, with the same semantics as ordinary builds.
    if let Ok(flags) = std::env::var("OPENVAF_LDFLAGS") {
        linker.cmd.args(flags.split(' ').filter(|flag| !flag.is_empty()));
    }
    Ok(Box::new(linker))
}

struct PortableLinker {
    cmd: Command,
    windows: bool,
}

impl Linker for PortableLinker {
    fn cmd(&mut self) -> &mut Command {
        &mut self.cmd
    }

    fn output_filename(&mut self, path: &Utf8Path) {
        if self.windows {
            self.cmd.arg(format!("/OUT:{path}"));
        } else {
            self.cmd.args(["-o", path.as_str()]);
        }
    }

    fn add_object(&mut self, path: &Utf8Path) {
        self.cmd.arg(path.as_str());
    }

    fn set_output_kind(&mut self) {}
}
