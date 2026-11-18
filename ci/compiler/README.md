# Portable OSDI 0.5 compiler candidates

Modified on 2026-11-18 in the personal `cdaunt/OpenVAF` fork: OSDI Jacobian
constness analysis, native linker discovery and portable compiler distribution.
This modified work is provided under the GNU GPL version 3; see `LICENSE`.
Bug and fix record: https://github.com/cdaunt/OpenVAF/issues/2.

The fork tracks upstream `mb-experimental`. The `OSDI 0.5 compiler artifacts`
workflow builds Linux x86_64 (Ubuntu 24.04 baseline), macOS arm64 (minimum macOS
13.0) and Windows x86_64 (Windows 10/Server 2022 or newer). It uploads candidates
for 30 days and does not publish releases. Pin a tested commit and archive
checksum; never implicitly select the newest branch build.

## Download and use

Download the matching platform artifact from a successful workflow run. Verify
its SHA-256 checksums, then extract the inner compiler ZIP, preserving executable
permissions on Unix. Keep its `bin`, `lib`, license and third-party directories
together. Run `bin/openvaf-r --target_cpu generic model.va -o model.osdi` (use
`bin/openvaf-r.exe` on Windows). No Pixi, Python, LLVM, MSYS2, GCC, Rust or Xcode
installation is required by the compiler archive. Normal platform system runtime
libraries remain prerequisites; this is a relocatable archive, not a static binary.

The marker `bin/openvaf-portable` selects native linking through the bundled LLD
using executable-relative paths. Portable mode supports the native target only.
Explicit `--linker` overrides retain upstream behavior for advanced toolchains;
cross-compilation is not qualified by these archives. macOS libraries resolve
system symbols through their loading host, without an embedded Apple SDK.
Windows import libraries are generated from the operating system UCRT exports.

## Build and verification

Producer CI uses the same checked-in `pixi.toml` and `pixi.lock` on all three
platforms, with conda-forge LLVM/Clang/LLD 21.1.8 and Rust 1.94.0. Run
`pixi install --locked` and `pixi run build`. Producer build machines still need
their normal platform build prerequisites (including the MSVC build environment
on Windows and developer tools on macOS); consumers do not.

Run `pixi run python ci/compiler/check_metadata.py target/release/openvaf-r
build/metadata-check` (append `.exe` on Windows). Package using
`pixi run python ci/compiler/package.py target/release/openvaf-r x86_64-linux`,
with the platform-appropriate binary and target. Packaging requires a committed,
clean tracked source tree and installed Pixi package caches for license capture.

The compiler ZIP contains source provenance, a dependency inventory, third-party
package/license/source recipe records, and affine/nonlinear OSDI 0.5 regression
fixtures. The separate source ZIP is the exact Git commit, including the fix and
build scripts. Keep it available alongside the binaries under the included GPL
terms. `pixi.lock` records dependency URLs and checksums.

CI extracts the exact archive to a path containing spaces and Unicode, hides its
producer Pixi environment, clears tool search paths, and compiles and loads both
regression models. The separate `*-portable-check.json` receipt identifies the
archive checksum tested. Only passing jobs upload artifacts. Full downstream
model-catalog numerical qualification and detached public-facade checks remain
separate from this focused compiler gate. Application developers consume prebuilt
`.osdi` packs and do not need these compiler archives.
