# Contributing to VRCS

Thank you for helping improve VRCS.

By contributing, you agree that your contributions are licensed under the
[GNU Affero General Public License v3.0](../LICENSE).

## Localization

Adding a language is intentionally a file-only contribution: create one locale JSON file, translate it, and submit it with the localization checks passing. The application discovers valid locale files automatically.

See the [localization contribution guide](LOCALIZATION.md) for the generator command, translation rules, validation, and review checklist.

## Development checks

For Windows development, install Node.js 24+, Rustup, Visual Studio Build Tools
with the **Desktop development with C++** workload, and CMake on `PATH`.
CPU and Vulkan builds do not require a GPU SDK or shader compiler. See
[Run from source](../README.md#development) for startup commands.
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
these builds do not sign installers or create releases. The workspace CI runs
formatting on Linux and CPU plus Vulkan Rust checks in one Windows job, and only
pushes to `main` save Rust caches.
CI validates locale resources, then runs the frontend tests and build.

Please keep commits focused and do not include generated build output.

### Linux development (initial adaptation)

VRCS publishes native Linux `x86_64` desktop packages (`.deb`, `.rpm`, and
`AppImage`) on GitHub Releases (ARM64 remains deferred for now), while full
hardware and desktop-session acceptance across all milestones is still ongoing.
`npm run dev` and `npm run dev:vulkan` dispatch through Node: Windows retains
PowerShell preparation; Linux does not stage Windows DLLs.
`npm run dev:vulkan` enables managed Qwen GPU recognition through the pinned native
Vulkan runtime; default development builds remain CPU-only. Install the system
Vulkan loader (`libvulkan1` on Ubuntu) and a compatible GPU driver, not a GPU SDK.
`npm run build` remains the Windows installer entry point.

For the Ubuntu 24.04 (`glibc 2.39+`) CI/release ABI baseline (also compatible with
Ubuntu 24.04+ and Debian 13+), install Node.js 24+ and the pinned Rust
toolchain, then run `bash scripts/install-linux-deps.sh`, `npm ci`, and
`npm run dev` in a graphical user session. Linux network capture additionally
needs `/usr/bin/vrcs-desktop` (with built-in `--network-helper`) and its polkit
policy, installed by the `.deb`/`.rpm` system package or manually as below.
`npm run dev` does not install or update either file; a build-tree binary is
intentionally not executed through pkexec. The dependency script includes
WebKitGTK 4.1/GTK3, PipeWire/SPA, D-Bus, AppIndicator, native build tools,
RPM inspection/packaging tools (`rpm`, `rpm2cpio`, `cpio`), CJK fonts, `libopus0`,
`pkexec` and `polkitd`. A running polkit authentication agent in the graphical
user session is also required (provided by many desktops; start your compositor's
configured agent on minimal Wayland sessions). It is a build prerequisite list,
not a portability guarantee.

For Arch Linux x86_64 development, install the network runtime prerequisites and
the desktop binary/policy from the repository root after setting up Rust:

```sh
sudo pacman -S --needed polkit opus
npm ci
npm run build:frontend
cargo build --locked -p vrcs-desktop --target-dir .build
sudo install -d -o root -g root -m 0755 /usr/bin /usr/share/polkit-1/actions
sudo install -o root -g root -m 0755 .build/debug/vrcs-desktop /usr/bin/vrcs-desktop
sudo install -o root -g root -m 0644 packaging/linux/app.vrcs.network-capture.policy /usr/share/polkit-1/actions/app.vrcs.network-capture.policy
```

Use the canonical `/usr/bin/vrcs-desktop` path: the policy authorizes that exact
executable, and `vrcs-desktop` refuses to run under `pkexec` without
`--network-helper`. Keep it and its parent directories root-owned and not
writable by group/others; do not symlink it to a user-writable checkout or install
it setuid. Run VRCS as your normal user and authorize the first capture through
the session's polkit agent. To update, stop capture and repeat the build and
installation commands from the matching application revision. To uninstall a
manual installation, stop capture and run:

```sh
sudo rm -f /usr/bin/vrcs-desktop /usr/share/polkit-1/actions/app.vrcs.network-capture.policy
```

For a `.deb`- or `.rpm`-managed installation, upgrade/remove the system package
with your package manager instead. AppImage installs neither file; use the
manual procedure or the `.deb`/`.rpm` package on supported distributions. There
is currently no separately published helper package. These steps install
prerequisites only: network capture remains experimental, and real VRChat/Proton
reception, authorization and sustained operation still require game acceptance.

Schema 43 retains Linux device keys and numeric device IDs while migrating legacy
player selections to per-account source policies. Upgrades from schema 41 or 42
keep a versioned configuration backup; an empty legacy selection remains blocked.
The standalone VRChat log watcher also runs on Linux, using the target process's
Wine/Proton prefix. Network capture and conversation world markers share its
snapshot, so log monitoring does not require starting the privileged helper.
Managed Qwen 0.6B and 1.7B models share the existing Linux CPU/Vulkan runtime;
the larger model does not require a separate runtime package.

Implemented boundaries:

- Authenticated Core `GET /api/capabilities` and Tauri `platform_capabilities`
  distinguish `supported`, `available`, and `reason_code`. Runtime/model files
  alone are not proof of feature availability.
- Native PipeWire microphone and sink-monitor capture use stable device keys.
  Schema 42 preserves numeric IDs and backs up schema 41. Missing/imported
  devices require reselection; they never silently fall back to another device.
  Once running, default-device capture rebuilds after device loss, stream
  disconnection or a PipeWire restart while retaining its PCM channel. Recovery
  waits remain cancellable; explicit selections and permanent errors still fail.
  Virtual microphone sources are selectable. A full transcription PCM queue does
  not invalidate audio already delivered to an independent visual-question tap.
  Tap-only visual questions close their unused PCM receiver to avoid queue growth
  and overflow warnings.
- Secret Service stores profile-isolated credentials; environment overrides
  retain precedence. Missing/locked services and cancelled prompts report errors;
  no plaintext credential fallback is introduced.
  Async callers offload keyring work. Matching entries are unlocked and snapshotted
  before mutation; partial failures attempt restoration and report restoration
  failures explicitly. Secret Service has no cross-process transaction, and a
  connection failure can prevent restoration from being confirmed.
- Managed Qwen CPU/Vulkan uses fixed llama.cpp b11501 assets with size/SHA checks,
  architecture-specific ELF validation, bounded archive extraction, internal-link
  checks, staged activation and child-process cleanup. Vulkan builds select an
  x86_64 Vulkan package including CPU backends (ARM64 is deferred for now);
  switching from a CPU build
  requires downloading the Vulkan runtime, not the ASR model again. Auto selects an
  available GPU and retains the existing CPU fallback on startup failure; explicit
  GPU selection reports failure rather than silently using CPU. Local PLaMo remains
  CPU-only on Linux GNU x86_64, sharing the selected runtime through a separate
  authenticated loopback process; its model and license consent remain independent.
  ARM64 PLaMo remains disabled (ARM64 releases are deferred).
  PipeWire captures microphone, system output, and VRChat process audio. Linux
  x86_64 VR subtitles, wrist overlays, dashboard, and controller input
  (OpenXR/OpenVR) are enabled by default and render directly through the native
  GPU UI renderer (`vr_overlay::gpu_ui`, without requiring a WebKitGTK VR
  surface host), while VR scene capture
  (`vr.scene_capture`, for VR OCR and VR visual question answering) remains
  disabled on Linux pending upstream OpenXR/Monado scene-capture APIs. Desktop
  OCR and desktop visual question answering via `org.freedesktop.portal.Screenshot`
  remain available, and OSC sending is independent. Linux GNU x86_64 now has
  helper-backed experimental player audio and Chatbox network reception. Both
  `.deb` and `.rpm` install `/usr/bin/vrcs-desktop` with built-in `--network-helper`
  support and its polkit policy in the same package; AppImage needs manual system
  installation or a `.deb`/`.rpm` package. No standalone helper package is
  currently published. Capture needs a session polkit agent and system Opus.
  Real VRChat/Proton traffic, authorization, packaging, anti-cheat behavior and
  long-running capture are not yet accepted; ARM64 is deferred for now.
  Candidate packets require a fresh inode/endpoint/target-fd confirmation; stale
  queued packets are discarded when the inventory changes. `/proc` reads are not
  atomic with packet arrival or later socket close/rebind, so this is not a
  packet-level ownership guarantee. Ambiguous same-port IPv6 sockets are rejected
  conservatively, including IPv6-only sockets whose mode cannot be established.
  A confirmed game exit ends its session and returns capture to waiting for the
  next game; authorization and startup failures also hand off child reaping.
- Desktop capture uses `org.freedesktop.portal.Screenshot` for interactive region
  selection and forwards the captured PNG to OCR/vision. On X11, global shortcuts
  use native XCB key grabs; on Wayland, GlobalShortcuts Portal registration is
  opt-in and independent from the button. The capability snapshot reports active
  bindings; changing their configuration or closing the Portal session invalidates
  authorization and requires rebinding.
- Data remains in Tauri's local data directory under `.vrcs`; logs use
  `$XDG_STATE_HOME/vrcs/logs` or `$HOME/.local/state/vrcs/logs`. The desktop data root
  is private; configuration, migration backups and database files are owner-only,
  including existing files when reopened. Wayland compact subtitles do not promise
  global stacking. Tray failure leaves close-to-exit behavior.
- `node scripts/build-linux-release.mjs` builds native deb/rpm/AppImage channels with
  Qwen Vulkan support; `--deb-only`, `--rpm-only`, and `--appimage-only` limit
  local packaging, `--preview` stages an unsigned directory with the same
  inference support, `--verify-packages` additionally checks deb/rpm packaging
  in preview mode, and `--max-glibc` enforces the ELF glibc version ceiling.
  Runtime dependencies include the managed llama.cpp libraries' system requirements
  (`libgomp1`, `libssl3`/`libssl3t64`/`libssl.so.3`, `libstdc++6`, `libgcc-s1`,
  Opus, and the Vulkan loader), which are not all visible in the desktop
  executable's ELF dependency list. `.rpm` packages declare cross-distro
  SONAME and `/usr/bin/pkexec` dependencies so one package covers Fedora,
  Nobara, Bazzite, RHEL-family 10, openSUSE Tumbleweed, and Mageia.
  `deb`/`rpm`/development installs cannot install updates in-app (`deb`/`rpm` can
  check for updates and guide manual/package-manager updates); writable AppImages
  support signed in-app updates when configured with the updater signing keys.
  Complete release aggregation verifies every platform's package, architecture,
  hash and signature before writing `latest.json`.
- CI/Preview/Release Linux jobs run on `ubuntu-24.04` (`x86_64`, `glibc 2.39`
  baseline); ARM64 (`aarch64` / `ubuntu-24.04-arm`) is temporarily removed until
  future enablement. Same-commit CI reuse checks `linux (x86_64)` alongside
  Windows/frontend jobs, and Release runs container install verification
  (`scripts/verify-linux-package.sh`) on Ubuntu 24.04, Fedora, and openSUSE
  Tumbleweed before publishing. Workflow configuration is not evidence that the
  remote jobs have run successfully. Changes to `packaging/linux/`, including
  polkit policy and installation scripts, select the native CI checks.

Measured evidence (2026-11-10 unless dated otherwise; x86_64 Xeon E3-1245 v6, Arch/niri Wayland unless
otherwise stated):

| Boundary | Exercised result | Not established |
| --- | --- | --- |
| Native desktop | WebKitGTK onboarding, settings and compact-window interaction; Core startup and configuration migration | GNOME/KDE desktop sessions, Windows regression, ARM64 native runtime |
| PipeWire 1.6.10 / WirePlumber 0.5.18 | Disposable graph at 48 kHz and 44.1 kHz; microphone and monitor each deliver 24×512 finite, nonzero mono 16 kHz samples; stop 23/28 ms; missing/numeric IDs and destroyed source rejected | Real devices, default switching/hotplug, 30-minute run, Ubuntu PipeWire baseline, ARM64 |
| PipeWire default recovery (2026-11-11) | Disposable graph: default source removal/recreation resumed microphone PCM; a live PipeWire restart resumed both microphone and sink-monitor PCM; explicit source loss still failed; both captures stopped within 1 s while defaults were absent | Physical device unplug/replug, desktop audio-service integration and ARM64 |
| Secret Service | Disposable D-Bus/GNOME keyring: two profiles, reconnect reads, update and idempotent deletion; lifecycle test passed | Real user unlock/cancel UI, desktop restart persistence on both architectures |
| Qwen CPU | Real pinned download/install/launch; 0.6B Q8 model transcribes the fixed JFK sample on both channels; forced-exit recovery and cleanup passed (104.52 s for the whole lifecycle test, not per-request latency) | ARM64 execution and sustained real-time performance |
| Qwen Vulkan / Linux GNU x86_64 | Pinned Vulkan runtime download, verification, installation and device enumeration passed; Intel HD Graphics P630 / Mesa 26.2.6 transcribed the JFK sample on both channels, then recovered after forced exit; the combined CPU/GPU lifecycle test passed in 45.15 s (not per-request latency) | ARM64 execution, AMD/NVIDIA drivers, sustained real-time performance and GPU-startup fallback hardware acceptance |
| PLaMo CPU / Linux GNU x86_64 | Actual revision-bound license consent, 573 MB download/SHA verification and translation preview API: en→ja 3.262 s cold (includes startup), ja→en 0.189 s, zh-Hans→zh-Hant 0.408 s; 16 translations alongside the real dual-channel Qwen recovery test, max 0.421 s; unload/reload, SIGKILL recovery, directory migration, corrupt-destination rollback and restart with unreachable download proxy passed; active child reaped on Core shutdown | ARM64 remains disabled; other CPUs and long-text throughput are not established |
| ONNX CPU | Silero v6.2.1 and Smart Turn prediction; PP-OCR multilingual two-image inference, cold 3.413 s / warm 1.617 s | ARM64 inference and performance |
| WebKit VR host experiment *(historical; replaced in 0.4.4 by the native GPU UI renderer)* | Real offscreen React reader: 16 accepted layout/paint-matched snapshots, pin click, 2 hidden updates, pause/resume and reload; CJK glyphs and transparent edges inspected; mean 6.240 ms / max 20.075 ms per snapshot | OpenVR submissions, headset visual quality, controller input and production VR |
| Ubuntu 24.04 x86_64 deb/AppImage | Both native packages built, extracted, dependencies resolved and first-start Silero session self-test passed; unsigned manifests; metadata signature/architecture rejection tests 10/10 passed | Clean-machine graphical install/upgrade, ARM64 package, signed publication; application ELF requires up to GLIBC_2.39 (AppImage does not remove this baseline) |
| ScreenCast Portal / niri 26.04 | Actual source authorization, linear DMA-BUF freeze, in-app region selection and local OCR completed; scan 2.560 s (including 2.005 s model load); capture closed before selection; authorization cancel/reopen and frozen-picker cancel removed the window and re-enabled capture | Cloud visual-question response, GNOME/KDE sessions, mixed displays/rotation, ARM64 and remaining cancellation matrix |
| GlobalShortcuts Portal / niri | Interface v2 was present, but actual authorization returned unavailable; the UI reported the limitation and screenshot buttons remained usable | Real binding/activation/replacement on a supported desktop backend |

On Linux `x86_64`, VR subtitles, wrist overlays, dashboard, and controller input
(OpenXR/OpenVR) are enabled by default in desktop builds; ARM64 reports
`vr.runtime_arch_unverified` and is deferred. VR scene capture (`vr.scene_capture`,
for VR OCR and VR visual question answering) remains disabled on Linux
(`vr.scene_capture_unavailable`) pending upstream OpenXR/Monado scene-capture APIs;
desktop OCR and desktop visual question answering via Screenshot Portal are not
blocked and remain available. M0/M3 headset acceptance still requires real hardware
verification.

The repeatable audio fixture is `core/tests/pipewire-smoke.conf`. Run it only in a
private `PIPEWIRE_RUNTIME_DIR` with `PIPEWIRE_REMOTE=vrcs-test-pipewire`, a private
WirePlumber policy instance and source-to-sink FL/FR links; then run the ignored
`isolated_pipewire_captures_both_sources_and_stops_without_audio` test. It destroys
its test source. Never run this against the user's audio session. The non-live
source follows a shared graph driver to avoid virtual-source timer starvation.

Current milestone gates:

| Milestone | Remaining acceptance |
| --- | --- |
| M0 | Real SteamVR/OpenXR static/dynamic headset submission; WebKit host experiment passed on this x86_64 desktop (ARM64 deferred) |
| M1 | Clean x86_64 deb/rpm install, real dual-source/cloud-ASR/translation/subtitle/VRChat OSC loop, restart persistence, GNOME X11/Wayland and long-running device tests |
| M2 | Broader x86_64 GPU-driver and sustained-inference acceptance (ARM64 deferred) |
| M3 | Real-headset acceptance for Linux x86_64 subtitles, wrist overlays, dashboard, independent controller input, and runtime/host recovery (enabled in default builds) |
| M4 | Portal end-to-end desktop selection/OCR/VQA and cancellation across target desktops, real global-shortcut binding/activation (VR scene capture / VR OCR / VR VQA remain disabled on Linux pending upstream OpenXR/Monado APIs) |
| M5 | Linux x86_64 AppImage/deb/rpm install/upgrade, real signed-update rejection, offline restart and read-only installation; KDE Wayland and remote CI results (ARM64 deferred) |

Missing prerequisites are SteamVR/OpenXR headset & controllers,
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

## Nightly releases

The **Nightly** workflow runs daily at 00:17 Beijing time. It builds the last
change to `main` from the preceding calendar day when that day had any changes.
The Windows x64 installer uses the existing release signing secrets. A successful
build is published as a `nightly-YYYYMMDD` GitHub pre-release, then its updater
manifest is committed to `nightly-updates`.

GitHub can require Workflows write permission to create a ref for an older
commit. Set the `NIGHTLY_REF_TOKEN` Actions secret to a fine-grained token
scoped to this repository with Contents and Workflows write access to support
those builds. Nightly uses it only when creating refs.
[GitHub ref permissions](https://docs.github.com/en/rest/git/refs#create-a-reference)

Run **Nightly** from `main` with a completed Beijing date to recover a missed or
failed run. A rerun can finish publishing a manifest from an existing release.
After merging the workflow, publish the first Nightly before publishing the
first stable build that exposes the Nightly switch.

## Player audio development

Experimental per-player audio capture and Chatbox network reception share the
network capture modules in `core/src/player_audio/` and
`crates/player-audio-scan/`. The capture module produces independent 16 kHz mono
PCM streams; each stream enters the shared recognition and translation pipeline
with its own identity and lifecycle.

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

The metadata check verifies that the updater manifest references the signed
standard installer and matching checksum. The application identifier and updater
signing configuration must remain compatible with existing installations.
