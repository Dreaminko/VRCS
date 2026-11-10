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
targets, and tests for the Core, desktop, and native scan crates. Neither CPU
nor Vulkan checks require an SDK.
Run it on Windows before submitting Rust changes so the desktop capture and
overlay code is checked. For Vulkan changes, also run
`.\scripts\check-rust.ps1 -Vulkan`; this stages the Vulkan loader and checks
the Core and desktop crates with Vulkan enabled. CI checks both CPU and Vulkan
builds.

Use `-Check format`, `-Check clippy`, or `-Check test` to run one Rust check
stage. Formatting does not prepare runtime files.
Release tags reuse the latest `main` push CI run for the same commit only when
it completed successfully and every required job passed. Partial, failed, cancelled,
or unavailable CI records trigger full validation. Installer builds run in
parallel with this validation. Release assets are uploaded only after packaging
and validation succeed. Relevant pushes to `main` warm the release dependency
cache when no exact cache exists;
these builds do not sign installers or create releases. CPU and Vulkan CI
caches are separate, and only pushes to `main` save Rust caches.
CI validates locale resources, then runs the frontend tests and build.

Please keep commits focused and do not include generated build output.

### Linux development (initial adaptation)

The `linux` branch contains native desktop adaptation, not a published or fully
accepted Linux release. `npm run dev` and `npm run dev:vulkan` dispatch through
Node: Windows retains PowerShell preparation; Linux does not stage Windows DLLs.
`npm run dev:vulkan` enables managed Qwen GPU recognition through the pinned native
Vulkan runtime; default development builds remain CPU-only. Install the system
Vulkan loader (`libvulkan1` on Ubuntu) and a compatible GPU driver, not a GPU SDK.
`npm run build` remains the Windows installer entry point.

For the Ubuntu 24.04 native build baseline, install Node.js 24+ and the pinned
Rust toolchain, then run `bash scripts/install-linux-deps.sh`, `npm ci`, and
`npm run dev` in a graphical user session. The dependency script includes
WebKitGTK 4.1/GTK3, PipeWire/SPA, D-Bus, AppIndicator, native build tools, and CJK
fonts. It is a build prerequisite list, not a portability guarantee.

Implemented boundaries:

- Authenticated Core `GET /api/capabilities` and Tauri `platform_capabilities`
  distinguish `supported`, `available`, and `reason_code`. Runtime/model files
  alone are not proof of feature availability.
- Native PipeWire microphone and sink-monitor capture use stable device keys.
  Schema 42 preserves numeric IDs and backs up schema 41. Missing/imported
  devices require reselection; they never silently fall back to another device.
- Secret Service stores profile-isolated credentials; environment overrides
  retain precedence. Missing/locked services and cancelled prompts report errors;
  no plaintext credential fallback is introduced.
- Managed Qwen CPU/Vulkan uses fixed llama.cpp b11501 assets with size/SHA checks,
  architecture-specific ELF validation, bounded archive extraction, internal-link
  checks, staged activation and child-process cleanup. Vulkan builds select separate
  x86_64/aarch64 Vulkan packages including CPU backends; switching from a CPU build
  requires downloading the Vulkan runtime, not the ASR model again. Auto selects an
  available GPU and retains the existing CPU fallback on startup failure; explicit
  GPU selection reports failure rather than silently using CPU. Local PLaMo remains
  CPU-only on Linux GNU x86_64, sharing the selected runtime through a separate
  authenticated loopback process; its model and license consent remain independent.
  ARM64 PLaMo remains disabled; ARM64 Qwen CPU/Vulkan execution is not yet verified.
  Application-only audio, experimental player audio, Chatbox network reception
  and key acquisition remain excluded; OSC sending is independent.
- ScreenCast Portal capture requests one user-selected source, receives one frame
  through its authorized PipeWire remote, closes capture, then opens an in-app
  frozen-frame picker. Only the crop reaches OCR/vision. Shared memory and packed
  single-plane linear DMA-BUF are supported; unsupported modifiers fail explicitly.
  GlobalShortcuts Portal registration is opt-in and independent from the button.
- Data remains in Tauri's local data directory under `.vrcs`; logs use
  `$XDG_STATE_HOME/vrcs/logs` or `$HOME/.local/state/vrcs/logs`. New directories are
  private. Wayland compact subtitles do not promise global stacking. Tray failure
  leaves close-to-exit behavior rather than hiding the only window.
- `node scripts/build-linux-release.mjs` builds native deb/AppImage channels with
  Qwen Vulkan support; `--deb-only` limits local packaging and `--preview` stages
  an unsigned directory with the same inference support.
  deb/development installs never use the AppImage updater. Publishable AppImages
  require the existing signing key and public key. Complete release aggregation
  verifies every platform's package, architecture, hash and signature before
  writing `latest.json`; Windows standard and legacy CUDA targets are retained.
- CI/Preview/Release have native `ubuntu-24.04` and `ubuntu-24.04-arm` jobs.
  Same-commit CI reuse requires both Linux jobs. Workflow configuration is not
  evidence that the remote jobs have run successfully.

Measured evidence (2026-11-10; x86_64 Xeon E3-1245 v6, Arch/niri Wayland unless
otherwise stated):

| Boundary | Exercised result | Not established |
| --- | --- | --- |
| Native desktop | WebKitGTK onboarding, settings and compact-window interaction; Core startup and configuration migration | GNOME/KDE desktop sessions, Windows regression, ARM64 native runtime |
| PipeWire 1.6.10 / WirePlumber 0.5.18 | Disposable graph at 48 kHz and 44.1 kHz; microphone and monitor each deliver 24×512 finite, nonzero mono 16 kHz samples; stop 23/28 ms; missing/numeric IDs and destroyed source rejected | Real devices, default switching/hotplug, 30-minute run, Ubuntu PipeWire baseline, ARM64 |
| Secret Service | Disposable D-Bus/GNOME keyring: two profiles, reconnect reads, update and idempotent deletion; lifecycle test passed | Real user unlock/cancel UI, desktop restart persistence on both architectures |
| Qwen CPU | Real pinned download/install/launch; 0.6B Q8 model transcribes the fixed JFK sample on both channels; forced-exit recovery and cleanup passed (104.52 s for the whole lifecycle test, not per-request latency) | ARM64 execution and sustained real-time performance |
| Qwen Vulkan / Linux GNU x86_64 | Pinned Vulkan runtime download, verification, installation and device enumeration passed; Intel HD Graphics P630 / Mesa 26.2.6 transcribed the JFK sample on both channels, then recovered after forced exit; the combined CPU/GPU lifecycle test passed in 45.15 s (not per-request latency) | ARM64 execution, AMD/NVIDIA drivers, sustained real-time performance and GPU-startup fallback hardware acceptance |
| PLaMo CPU / Linux GNU x86_64 | Actual revision-bound license consent, 573 MB download/SHA verification and translation preview API: en→ja 3.262 s cold (includes startup), ja→en 0.189 s, zh-Hans→zh-Hant 0.408 s; 16 translations alongside the real dual-channel Qwen recovery test, max 0.421 s; unload/reload, SIGKILL recovery, directory migration, corrupt-destination rollback and restart with unreachable download proxy passed; active child reaped on Core shutdown | ARM64 remains disabled; other CPUs and long-text throughput are not established |
| ONNX CPU | Silero v6.2.1 and Smart Turn prediction; PP-OCR multilingual two-image inference, cold 3.413 s / warm 1.617 s | ARM64 inference and performance |
| WebKit VR host experiment | Real offscreen React reader: 16 accepted layout/paint-matched snapshots, pin click, 2 hidden updates, pause/resume and reload; CJK glyphs and transparent edges inspected; mean 6.240 ms / max 20.075 ms per snapshot | OpenVR submissions, headset visual quality, controller input and production VR |
| Ubuntu 24.04 x86_64 deb/AppImage | Both native packages built, extracted, dependencies resolved and first-start Silero session self-test passed; unsigned manifests; metadata signature/architecture rejection tests 10/10 passed | Clean-machine graphical install/upgrade, ARM64 package, signed publication; application ELF requires up to GLIBC_2.39 (AppImage does not remove this baseline) |
| ScreenCast Portal / niri 26.04 | Actual source authorization, linear DMA-BUF freeze, in-app region selection and local OCR completed; scan 2.560 s (including 2.005 s model load); capture closed before selection; authorization cancel/reopen and frozen-picker cancel removed the window and re-enabled capture | Cloud visual-question response, GNOME/KDE sessions, mixed displays/rotation, ARM64 and remaining cancellation matrix |
| GlobalShortcuts Portal / niri | Interface v2 was present, but actual authorization returned unavailable; the UI reported the limitation and screenshot buttons remained usable | Real binding/activation/replacement on a supported desktop backend |

The WebKit experiment is available through:

```sh
cargo run --locked --manifest-path apps/desktop/src-tauri/Cargo.toml --features vr-preview --bin vrcs-linux-vr-prototype -- --mode web --assets apps/desktop/dist --seconds 16
```

`--mode static` independently exercises OpenVR raw submission; `--vr` adds it to
the dynamic experiment. A successful offscreen PNG is not a headset test. The
production Linux VR capability remains unavailable; ARM64 reports
`vr.runtime_arch_unverified`. VR mirror capture is not replaced by a desktop
Portal image. M0's static/dynamic hardware gate must pass before production M3.

The repeatable audio fixture is `core/tests/pipewire-smoke.conf`. Run it only in a
private `PIPEWIRE_RUNTIME_DIR` with `PIPEWIRE_REMOTE=vrcs-test-pipewire`, a private
WirePlumber policy instance and source-to-sink FL/FR links; then run the ignored
`isolated_pipewire_captures_both_sources_and_stops_without_audio` test. It destroys
its test source. Never run this against the user's audio session. The non-live
source follows a shared graph driver to avoid virtual-source timer starvation.

Current milestone gates:

| Milestone | Remaining acceptance |
| --- | --- |
| M0 | Native ARM64 build evidence and real SteamVR static/dynamic submission; WebKit host experiment passed only on this x86_64 desktop |
| M1 | Both architectures' clean deb install, real dual-source/cloud-ASR/translation/subtitle/VRChat OSC loop, restart persistence, GNOME X11/Wayland and long-running device tests |
| M2 | ARM64 CPU/Vulkan model execution/performance, including CPU PLaMo; broader GPU-driver and sustained-inference acceptance |
| M3 | Hardware-gated production subtitles, independent controller input, wrist/dashboard and runtime/host recovery; not enabled |
| M4 | Portal end-to-end selection and cancellation across target desktops/architectures, real global-shortcut binding/activation, independent VR mirror result |
| M5 | Both architectures' AppImage/deb install/upgrade, real signed-update rejection, offline restart and read-only installation; KDE Wayland and remote CI results |

Missing prerequisites are an ARM64 native host, SteamVR/headset/controllers,
configured provider credentials for real cloud requests, and release signing and
publication access. No milestone's complete exit condition is claimed. Keep the
original schema-41 backup with its matching binary when rolling back; do not
delete history, models or keyring entries to roll back the application.

Run the narrow check for the changed boundary. Core and desktop strict all-target
clippy, desktop image and shared-protocol tests, frontend build and four-locale
validation passed locally. They do not replace desktop-session or hardware acceptance.

## Preview builds

Run the **Preview** workflow from GitHub Actions and select the branch to test.
Download its `VRCS-preview-windows-x64-<commit>` artifact, extract the complete
folder, and run `VRCS-preview.exe`. The artifact includes runtime DLLs, license
notices, and the source commit. It expires after seven days.

Preview uses a debug build with CPU and Vulkan support. It does not use signing
keys, generate an installer, publish a release, or enable automatic updates.
It requires Windows 10/11 x64, WebView2 Runtime, and Microsoft Visual C++ x64
Runtime. Close VRCS before starting it: the preview uses the existing VRCS data
directory. Each preview passes the startup self-test from its artifact folder
before upload; the full CI suite runs separately.

## Player audio development

The [player audio guide](docs/experimental-player-audio.md) describes setup,
player selection, subtitle visibility, and the main modules. The capture module
produces independent 16 kHz mono PCM streams; each stream enters the shared
recognition and translation pipeline with its own identity and lifecycle.

Run the standard development checks above for native or pipeline changes.
The player-audio browser fixtures cover the audio settings, permission flow,
memory compression controls, player selection, and desktop/VR captions:

```powershell
node --test apps/desktop/tests/player-*.browser.mjs apps/desktop/tests/memory-compression.browser.mjs
```

These fixtures require Chrome or Edge and use simulated native APIs. Regular
unit tests use synthetic packets and local service mocks. Live capture,
pagefile access, and local capture replay are opt-in tests with separate
requirements stated in their `#[ignore]` descriptions.

Use synthetic text, player identities, audio, and packets in committed fixtures.
Do not copy private conversation text from screenshots or recordings into tests.
Tests that exercise service requests should use loopback mocks and fake
credentials; real captures belong only in the opt-in local replay workflow.

### Local capture replay

Replay a local capture with its matching session key and an installed Opus
library. Each input line has three comma-separated fields: direction (`S` for
incoming traffic), capture metadata, and the hexadecimal encrypted packet.
Keep captures and keys outside version control and supply paths and keys through
process-local environment variables:

```powershell
$env:VRCS_TEST_CAPTURE = '<absolute path to a local capture>'
$env:VRCS_TEST_PHOTON_KEY = '<matching 64-character hexadecimal key>'
$env:VRCS_TEST_OPUS_DLL = '<absolute path to opus.dll>'
try {
    cargo test --manifest-path core/Cargo.toml replay_local_capture -- --ignored --nocapture
} finally {
    Remove-Item Env:VRCS_TEST_CAPTURE, Env:VRCS_TEST_PHOTON_KEY, Env:VRCS_TEST_OPUS_DLL -ErrorAction SilentlyContinue
}
```

The replay reports packet and player-frame counts, identity mappings, and PCM
SHA-256 digests. It does not write raw audio. Set `VRCS_TEST_REQUIRE_VOICE=1` to
require incoming voice, or `VRCS_TEST_REQUIRE_NAMES=1` to require identity
coverage for every speaking actor.

### Scan performance and diagnostics

Synthetic benchmarks measure scanning work without reading a real pagefile:

```powershell
cargo test --manifest-path crates/player-audio-scan/Cargo.toml benchmark_scan_8455_mib -- --ignored --nocapture
cargo test --manifest-path core/Cargo.toml benchmark_parallel_pagefile_8455_mib -- --ignored --nocapture
```

The scan-crate benchmark measures search and authentication. The Core benchmark
also includes parallel reads from a synthetic volume and buffer cleanup. Neither
measures physical disk I/O or Windows paging, so these timings do not represent
end-to-end key acquisition in a running game.

The `/health` response exposes `player_audio` counters for authenticated packets,
PCM blocks, dropped frames, and decode errors. Its `key_scan` object reports the
stage, elapsed time, scanned bytes, candidate count, and worker count.
`elapsed_ms` covers acquisition or cached-key authentication; `scan_elapsed_ms`
covers the scan itself. Waiting for the game, world, or network samples is
excluded. The `streaming` phase means the key is authenticated and capture is
ready; PCM and subtitle output must be checked separately.

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
