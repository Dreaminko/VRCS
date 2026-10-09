# Contributing to VRCS

Thank you for helping improve VRCS.

By contributing, you agree that your contributions are licensed under the
[GNU Affero General Public License v3.0](LICENSE).

## Localization

Adding a language is intentionally a file-only contribution: create one locale JSON file, translate it, and submit it with the localization checks passing. The application discovers valid locale files automatically.

See the [localization contribution guide](LOCALIZATION.md) for the generator command, translation rules, validation, and review checklist.

## Development checks

For Windows development, install Node.js 24+, Rustup, Visual Studio Build Tools
with the **Desktop development with C++** workload, and CMake on `PATH`.
CPU and Vulkan builds do not require a GPU SDK or shader compiler. See
[Run from source](README.md#run-from-source) for startup commands.
Run the commands below from the repository root.

Rust uses the version and components in `rust-toolchain.toml`. Rustup installs
them when you run a Rust command from this repository. CI and release builds
use the same file. Change the pinned version only after the Windows checks pass
with the new toolchain.

Install dependencies once:

```powershell
npm install
```

Start the desktop app with `npm run dev`, or the standalone Core with
`npm run dev:core`. These builds support cloud features and local CPU
Qwen recognition. Contributors who work on cloud services, translation, or
the interface can use this development setup without GPU tools.

Run the checks relevant to your change:

```powershell
npm run check:i18n
npm --workspace apps/desktop test
npm run build:frontend
.\scripts\check-rust.ps1
```

The Rust script runs formatting checks, Clippy with `-D warnings` for all
targets, and tests for both crates. Neither CPU nor Vulkan checks require
an SDK.
Run it on Windows before submitting Rust changes so the desktop capture and
overlay code is checked. For Vulkan changes, also run
`.\scripts\check-rust.ps1 -Vulkan`; this stages the Vulkan loader and checks
both crates with Vulkan enabled. CI checks both CPU and Vulkan builds.

Use `-Check format`, `-Check clippy`, or `-Check test` to run one Rust check
stage. Formatting does not prepare runtime files.
Release tags run the full CI workflow before packaging the standard installer.
Localization runs validate locale resources; CI runs the frontend tests and build.

Please keep commits focused and do not include generated build output.

## Qwen runtime and native builds

Development builds use CPU recognition by default. To enable Vulkan in the
desktop app, run `npm run dev:vulkan`. The standard release installer supports
CPU and Vulkan. The Vulkan feature selects how the independently downloaded
Qwen runtime can run; it does not compile GPU shaders in VRCS.

`scripts/prepare-vulkan-runtime.ps1` downloads and verifies the pinned Vulkan
loader and license notices. It stages them in application resources and local
build directories. Desktop Vulkan commands, release builds, and Vulkan Rust
checks run this script automatically. Initial preparation requires internet
access. It does not change the registry or system `PATH`.

For direct Cargo commands with Vulkan enabled, stage the loader first:

```powershell
& .\scripts\prepare-vulkan-runtime.ps1
cargo run --manifest-path core/Cargo.toml --features vulkan
```

For a CPU-only Core build, use the default features or:

```powershell
cargo run --manifest-path core/Cargo.toml --no-default-features
```

Qwen in automatic device mode uses CPU in these builds. An explicit GPU
selection reports that the Vulkan backend is unavailable. With Vulkan enabled,
automatic device mode prefers a compatible GPU and can fall back to CPU.

The desktop app downloads the Qwen runtime on demand. Standalone Core users
who need managed Qwen must run `& .\scripts\prepare-qwen-runtime.ps1` first
and install the model package. A compatible GPU and graphics driver are
required for Vulkan execution.

To check the build scripts without compiling Rust or downloading runtimes, run:

```powershell
.\scripts\test-check-rust.ps1
.\scripts\test-release-metadata.ps1
```

The metadata check verifies that standard and legacy CUDA updater targets
use one signed standard installer. The application identifier and updater
signing configuration must remain compatible with existing installations.
