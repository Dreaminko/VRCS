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
Vulkan SDK and Ninja are optional for development. You need them only when
you enable Vulkan acceleration or build release packages. See
[Run from source](README.md#run-from-source) for startup commands and CUDA
requirements. Run the commands below from the repository root.

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
recognition. They do not enable Vulkan or CUDA and do not download the
Vulkan SDK. Contributors who work on cloud services, translation, or the
interface can use this development setup without GPU SDKs.

Run the checks relevant to your change:

```powershell
npm run check:i18n
npm --workspace apps/desktop test
npm run build:frontend
.\scripts\check-rust.ps1
```

The Rust script runs formatting checks, Clippy with `-D warnings` for all
targets, and tests for both crates. It does not need the Vulkan SDK by default.
Run it on Windows before submitting Rust changes so the desktop capture and
overlay code is checked. For Vulkan changes, also run
`.\scripts\check-rust.ps1 -Vulkan`; this prepares the SDK and checks both
crates with Vulkan enabled. CI checks both CPU and Vulkan builds. Frontend
and localization checks do not require the Vulkan SDK.

Please keep commits focused and do not include generated build output.

## Vulkan SDK and native builds

Development builds use CPU recognition by default. To enable Vulkan in the
desktop app, run `npm run dev:vulkan`. CUDA development commands also enable
Vulkan and require both Vulkan tools and the CUDA Toolkit. Standard and CUDA
release packages retain Vulkan acceleration. Ninja must be on `PATH` before
SDK preparation. The preparation script selects Ninja for CMake to avoid
Windows path limits in the native shader build.

`scripts/prepare-vulkan-sdk.ps1` reuses the SDK at `VULKAN_SDK` if it contains
`Bin/glslc.exe`. Otherwise, it downloads SDK `1.4.309.0`, checks its SHA-256,
and copies it into `core/.cache/vulkan-sdk/1.4.309.0`. Initial preparation requires
internet access. It does not change the registry or system `PATH`.

Desktop Vulkan and CUDA commands (`npm run dev:vulkan` and `npm run dev:cuda`),
standalone CUDA development (`npm run dev:core:cuda`), release builds
(`npm run build`), and `scripts/check-rust.ps1 -Vulkan` prepare the SDK
automatically. Desktop development also uses `core/target` unless
`CARGO_TARGET_DIR` is set, to keep Vulkan shader build paths short.

Direct Cargo builds do not prepare the SDK. Run this script in the same
PowerShell session before Cargo commands that enable Vulkan:

```powershell
& .\scripts\prepare-vulkan-sdk.ps1
cargo run --manifest-path core/Cargo.toml --features vulkan
```

It sets `VULKAN_SDK`, adds the SDK `Bin` directory to `PATH`, sets
`CMAKE_GENERATOR=Ninja`, and stages the Vulkan loader for local executables and
tests. These environment variables apply only to the current shell. Repeat the
command in each new shell when you enable Vulkan. For direct desktop Cargo
builds, use a short absolute `CARGO_TARGET_DIR`, such as this repository's
`core/target` directory.

To build the Core with CPU-only Whisper and local Qwen, use:

```powershell
cargo run --manifest-path core/Cargo.toml --no-default-features
```

This is also the default Core build. It disables Vulkan acceleration for
both Whisper and managed local Qwen. Qwen in automatic device mode uses CPU;
an explicit GPU selection reports that
the Vulkan backend is unavailable. The desktop app downloads the Qwen runtime
on demand. Standalone Core users who need managed Qwen must run
`& .\scripts\prepare-qwen-runtime.ps1` first.

Users of the packaged app do not need the Vulkan SDK. The installer includes the
Vulkan loader. The app downloads the Qwen runtime on demand. Vulkan acceleration
requires a compatible GPU and graphics driver; CUDA acceleration requires the
additional runtime and driver listed in the README.
