# OSDI 0.5 compiler candidates

The personal `cdaunt/OpenVAF` fork tracks upstream `mb-experimental`, with the
Jacobian constness fix maintained as source changes. The `OSDI 0.5 compiler
artifacts` workflow builds Linux x86_64 (Ubuntu 24.04), macOS arm64 (deployment
minimum 13.0) and Windows x86_64 (MSYS2 UCRT64). It does not publish releases.

Each passing job uploads a compiler archive, the exact corresponding source
archive and SHA-256 checksums, retained for 30 days. Download from the successful
workflow run's Artifacts section; select the platform and exact commit. Keep both
archives together. GPLv3 terms are in `LICENSE`; the source archive includes the
changes and build scripts, with dependencies pinned by `Cargo.lock` and the
Windows LLVM package checksums. Upstream sources and copyright notices remain
intact.

These initial candidates contain the compiler executable, not a self-contained
LLVM toolchain. LLVM 21 shared libraries and their platform dependencies must be
installed and discoverable by the platform loader. See `runtime-dependencies.txt`
for the actual linked libraries. Linux requires a system linker (`ld`); macOS
requires Xcode Command Line Tools (`clang`/SDK); Windows uses MSYS2 UCRT64 and its
GNU linker toolchain. Windows must run in the same UCRT64 environment used by the
workflow. Do not treat these artifacts as a replacement for LLVM provisioning
until a portable compiler distribution is separately implemented and tested.

The workflow pins Rust 1.94.0 and LLVM major 21; Windows LLVM packages are additionally
checksum-pinned. For exact commands, see `.github/workflows/compiler-artifacts.yml`.
Build with `cargo +1.94.0 build --locked --release -p openvaf-driver --features llvm21`.
Then run `python ci/compiler/check_metadata.py target/release/openvaf-r build/metadata-check`
(use `openvaf-r.exe` on Windows). The metadata check compiles with a generic target
CPU and checks OSDI 0.5 exports, parameter-dependent affine F/Q Jacobian constness,
and preservation of nonconstant flags for nonlinear F/Q.

The fixture check is a compiler regression gate. Circulox's complete model catalog,
numerical qualification, detached public-facade consumer and model archive audits
remain separate. A downstream consumer must pin a tested commit/archive checksum;
never resolve a moving branch or the newest workflow run implicitly. Application
developers consume prebuilt `.osdi` model packs and do not need this compiler.
