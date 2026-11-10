import { cpSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { writeTargetManifest } from "./release-metadata.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
process.chdir(root);
const arch = { x64: "x86_64", arm64: "aarch64" }[process.arch];
if (process.platform !== "linux" || !arch) throw new Error("Linux releases require native Linux x86_64 or aarch64");
const host = execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/^host: (.+)$/m)?.[1];
const triple = `${arch}-unknown-linux-gnu`;
if (host !== triple) throw new Error(`Native Rust host ${triple} required; got ${host}`);
const preview = process.argv.includes("--preview");
const requireSigned = process.argv.includes("--require-signed");
const signed = Boolean(process.env.TAURI_SIGNING_PRIVATE_KEY?.trim());
if ((requireSigned || signed) && !process.env.TAURI_UPDATER_PUBLIC_KEY?.trim()) throw new Error("Updater public key required for signed release");
if (requireSigned && !signed) throw new Error("Updater private key required for a publishable release");
const version = JSON.parse(readFileSync("apps/desktop/src-tauri/tauri.conf.json", "utf8")).version;
if (process.env.GITHUB_REF?.startsWith("refs/tags/") && process.env.GITHUB_REF_NAME !== version) throw new Error("Release tag does not match application version");
for (const path of ["core/Cargo.toml", "apps/desktop/src-tauri/Cargo.toml"]) {
  if (readFileSync(path, "utf8").match(/^version = "([^"]+)"/m)?.[1] !== version) throw new Error(`Release version mismatch: ${path}`);
}
if (JSON.parse(readFileSync("apps/desktop/package.json", "utf8")).version !== version) throw new Error("Desktop package version mismatch");
const targetRoot = resolve(process.env.CARGO_TARGET_DIR || ".build");
const profile = preview ? "debug" : "release";
const binaryRoot = join(targetRoot, triple, profile);
const artifactRoot = resolve(preview ? "preview-artifacts" : "release-artifacts");
mkdirSync(artifactRoot, { recursive: true });
const work = mkdtempSync(join(tmpdir(), "vrcs-linux-package-"));
const generated = join(root, "apps/desktop/src-tauri/tauri.linux.generated.conf.json");
const env = { ...process.env, CARGO_TARGET_DIR: targetRoot, NO_STRIP: "1", APPIMAGE_EXTRACT_AND_RUN: "1" };
const run = (command, args, options = {}) => execFileSync(command, args, { cwd: root, env, stdio: "inherit", ...options });
const tauri = (args) => run("npm", ["--workspace", "apps/desktop", "run", "tauri", "--", ...args]);
function files(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((item) => {
    const path = join(directory, item.name);
    return item.isDirectory() ? files(path) : [path];
  });
}
function dependencies(binary) {
  const result = execFileSync("ldd", [binary], { encoding: "utf8", env });
  if (result.includes("not found")) throw new Error(`Missing dynamic dependency:\n${result}`);
  return { text: result, paths: [...result.matchAll(/(?:=>\s+|^\s*)(\/\S+)/gm)].map((match) => match[1]) };
}
function collectRuntime(binary, destination) {
  mkdirSync(destination, { recursive: true });
  const initial = dependencies(binary);
  // Ship non-system runtime libraries from Cargo / downloaded inference runtimes.
  // System GTK/WebKit/PipeWire and GPU drivers remain distribution dependencies.
  const pending = [binary];
  const seen = new Set();
  while (pending.length) {
    const current = pending.pop();
    for (const path of dependencies(current).paths) {
      const resolved = realpathSync(path);
      if (seen.has(resolved)) continue;
      seen.add(resolved);
      if (!resolved.startsWith("/usr/lib/") && !resolved.startsWith("/lib/")) {
        const output = join(destination, basename(path));
        copyFileSync(resolved, output);
        run("patchelf", ["--set-rpath", "$ORIGIN", output]);
        pending.push(resolved);
      }
    }
  }
  // ort's CPU builds can statically link ONNX; provider DSOs, when present,
  // are dlopened and do not appear in ldd. Only collect the active build profile.
  for (const path of readdirSync(binaryRoot).filter((name) => /^libonnxruntime.*\.so(?:\.|$)/.test(name))) {
    const output = join(destination, path);
    copyFileSync(realpathSync(join(binaryRoot, path)), output);
    run("patchelf", ["--set-rpath", "$ORIGIN", output]);
  }
  for (const path of files(destination)) dependencies(path);
  return initial.text;
}
function selfTest(binary, cwd) {
  // No LD_LIBRARY_PATH: this must also work when launched outside a shell.
  const cleanEnv = { ...env };
  delete cleanEnv.LD_LIBRARY_PATH;
  execFileSync("dbus-run-session", ["--", "xvfb-run", "-a", binary, "--release-self-test"], { cwd, env: cleanEnv, stdio: "inherit", timeout: 180_000 });
}
try {
  for (const channel of preview ? ["development"] : process.argv.includes("--deb-only") ? ["deb"] : ["deb", "appimage"]) {
    env.VRCS_PACKAGE_CHANNEL = channel;
    const config = JSON.parse(readFileSync("apps/desktop/src-tauri/tauri.linux.release.conf.json", "utf8"));
    config.bundle.active = !preview;
    config.bundle.targets = preview ? ["deb"] : [channel];
    config.bundle.createUpdaterArtifacts = channel === "appimage" && signed;
    if (signed) config.plugins = { updater: { pubkey: process.env.TAURI_UPDATER_PUBLIC_KEY, endpoints: ["https://github.com/Dreaminko/VRCS/releases/latest/download/latest.json"] } };
    writeFileSync(generated, JSON.stringify(config));
    tauri(["build", "--target", triple, "--features", "vulkan", "--config", "src-tauri/tauri.linux.generated.conf.json", "--no-bundle", "--no-sign", ...(preview ? ["--debug"] : []), "--", "--locked"]);
    const executable = join(binaryRoot, "vrcs-desktop");
    const stage = join(work, channel);
    const stageBin = join(stage, "usr/bin/vrcs-desktop");
    const stageLib = join(stage, "usr/lib/vrcs");
    mkdirSync(dirname(stageBin), { recursive: true });
    copyFileSync(executable, stageBin);
    const linked = collectRuntime(executable, stageLib);
    run("patchelf", ["--set-rpath", "$ORIGIN/../lib/vrcs:$ORIGIN/../lib:$ORIGIN/lib", stageBin]);
    writeFileSync(join(artifactRoot, `linux-${arch}-${channel}.ldd.txt`), linked);
    // These libraries are also loaded by name (not necessarily visible in ldd).
    const systemPackages = new Set(["libayatana-appindicator3-1", "libpipewire-0.3-0", "libdbus-1-3", "libwebkit2gtk-4.1-0", "libgtk-3-0t64", "libvulkan1"]);
    for (const library of dependencies(executable).paths) {
      if (!library.startsWith("/lib/") && !library.startsWith("/usr/lib/")) continue;
      let found = false;
      const resolved = realpathSync(library);
      for (const candidate of new Set([library, resolved, resolved.replace(/^\/usr\/lib\//, "/lib/"), library.replace(/^\/lib\//, "/usr/lib/")])) {
        try {
          const owners = execFileSync("dpkg-query", ["--search", candidate], { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
          for (const line of owners.trim().split("\n")) systemPackages.add(line.slice(0, line.indexOf(": ")).replace(/:(amd64|arm64)$/, ""));
          found = true;
          break;
        } catch { /* merged-/usr aliases can differ from the dpkg path */ }
      }
      if (!found) throw new Error(`No Ubuntu runtime package owns ${library}`);
    }
    const report = { os: "Ubuntu 24.04", arch, channel, host, systemPackages: [...systemPackages].sort(), bundledLibraries: readdirSync(stageLib).sort(), onnx: "pending-self-test" };
    selfTest(stageBin, stage);
    report.onnx = "Silero-v6.2.1-session-loaded";
    writeFileSync(join(artifactRoot, `linux-${arch}-${channel}.runtime.json`), `${JSON.stringify(report, null, 2)}\n`);
    if (preview) {
      cpSync(stage, join(artifactRoot, `VRCS-preview-linux-${arch}`), { recursive: true });
      copyFileSync("LICENSE", join(artifactRoot, `VRCS-preview-linux-${arch}/LICENSE`));
      copyFileSync("THIRD_PARTY_NOTICES.md", join(artifactRoot, `VRCS-preview-linux-${arch}/THIRD_PARTY_NOTICES.md`));
      writeFileSync(join(artifactRoot, `VRCS-preview-linux-${arch}/README.txt`), "Unsigned development preview. Run usr/bin/vrcs-desktop with the entire directory intact. Requires Ubuntu 24.04 system libraries listed in the adjacent runtime report and CJK fonts. This CI native ONNX self-test is not desktop/hardware acceptance or a published release. No automatic updates.\n");
      continue;
    }
    copyFileSync(stageBin, executable);
    const mapped = Object.fromEntries(readdirSync(stageLib).map((name) => [`/usr/lib/vrcs/${name}`, join(stageLib, name)]));
    config.bundle.linux.deb.files = mapped;
    config.bundle.linux.deb.depends = [...systemPackages].sort();
    config.bundle.linux.appimage.files = Object.fromEntries(readdirSync(stageLib).map((name) => [`usr/lib/vrcs/${name}`, join(stageLib, name)]));
    writeFileSync(generated, JSON.stringify(config));
    const bundleDirectory = join(binaryRoot, "bundle", channel);
    rmSync(bundleDirectory, { recursive: true, force: true });
    tauri(["bundle", "--target", triple, "--config", "src-tauri/tauri.linux.generated.conf.json", "--bundles", channel, ...(signed && channel === "appimage" ? [] : ["--no-sign"])]);
    const suffix = channel === "deb" ? ".deb" : ".AppImage";
    const packages = files(bundleDirectory).filter((path) => path.endsWith(suffix));
    if (packages.length !== 1) throw new Error(`Expected exactly one ${channel} package`);
    const output = join(artifactRoot, `VRCS-${version}-linux-${arch}${suffix}`);
    rmSync(`${output}.sig`, { force: true });
    copyFileSync(packages[0], output);
    if (channel === "appimage" && signed) copyFileSync(`${packages[0]}.sig`, `${output}.sig`);
    writeTargetManifest({ root: artifactRoot, version, os: "linux", arch, channel });
    const extracted = join(work, `extracted-${channel}`);
    mkdirSync(extracted);
    if (channel === "deb") run("dpkg-deb", ["--extract", output, extracted]);
    else run("chmod", ["+x", output]);
    if (channel === "appimage") run(output, ["--appimage-extract"], { cwd: extracted });
    const packagedBinary = join(extracted, ...(channel === "appimage" ? ["squashfs-root"] : []), "usr/bin/vrcs-desktop");
    dependencies(packagedBinary);
    selfTest(packagedBinary, dirname(packagedBinary));
  }
} finally {
  rmSync(generated, { force: true });
  rmSync(work, { recursive: true, force: true });
}
