import { cpSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { checkProjectVersion, writeTargetManifest } from "./release-metadata.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
process.chdir(root);
const arch = { x64: "x86_64" }[process.arch];
if (process.platform !== "linux" || !arch) throw new Error("Linux releases currently require native Linux x86_64");
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
checkProjectVersion(version);
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
  const copiedOrt = new Map();
  const ortEntries = readdirSync(binaryRoot).filter((name) => /^libonnxruntime.*\.so(?:\.|$)/.test(name));
  for (const entry of ortEntries) {
    const full = join(binaryRoot, entry);
    const resolved = realpathSync(full);
    if (!copiedOrt.has(resolved)) {
      const targetName = basename(resolved);
      const output = join(destination, targetName);
      copyFileSync(resolved, output);
      run("patchelf", ["--set-rpath", "$ORIGIN", output]);
      copiedOrt.set(resolved, targetName);
    }
  }
  for (const entry of ortEntries) {
    const targetName = copiedOrt.get(realpathSync(join(binaryRoot, entry)));
    if (entry !== targetName) {
      symlinkSync(targetName, join(destination, entry));
    }
  }
  for (const path of files(destination)) dependencies(path);
  return initial.text;
}
const DEB_DEPENDS = [
  "libayatana-appindicator3-1 | libappindicator3-1",
  "libc6",
  "libcairo2",
  "libdbus-1-3",
  "libgcc-s1",
  "libgdk-pixbuf-2.0-0",
  "libglib2.0-0t64 | libglib2.0-0",
  "libgomp1",
  "libgtk-3-0t64 | libgtk-3-0",
  "libopus0",
  "libpipewire-0.3-0t64 | libpipewire-0.3-0",
  "libsoup-3.0-0",
  "libstdc++6",
  "libvulkan1",
  "libwebkit2gtk-4.1-0",
  "pkexec | policykit-1",
];
const ALLOWED_SYSTEM_SONAMES = new Set([
  "ld-linux-x86-64.so.2",
  "libc.so.6",
  "libcairo.so.2",
  "libdbus-1.so.3",
  "libdl.so.2",
  "libgcc_s.so.1",
  "libgdk-3.so.0",
  "libgdk_pixbuf-2.0.so.0",
  "libgio-2.0.so.0",
  "libglib-2.0.so.0",
  "libgobject-2.0.so.0",
  "libgtk-3.so.0",
  "libjavascriptcoregtk-4.1.so.0",
  "libm.so.6",
  "libpipewire-0.3.so.0",
  "libpthread.so.0",
  "librt.so.1",
  "libsoup-3.0.so.0",
  "libstdc++.so.6",
  "libwebkit2gtk-4.1.so.0",
]);
function verifyDirectDependencies(binary, bundledLibraries) {
  const dynamic = execFileSync("readelf", ["-d", binary], { encoding: "utf8", env });
  const bundled = new Set(bundledLibraries);
  const unexpected = [];
  for (const line of dynamic.split("\n")) {
    const match = line.match(/\(NEEDED\)\s+Shared library:\s+\[([^\]]+)\]/);
    if (!match) continue;
    const soname = match[1];
    if (!ALLOWED_SYSTEM_SONAMES.has(soname) && !bundled.has(soname)) unexpected.push(soname);
  }
  if (unexpected.length) {
    throw new Error(`Unexpected direct DT_NEEDED dependencies in ${binary}: ${unexpected.join(", ")}`);
  }
}
function hostOs() {
  try {
    const release = readFileSync("/etc/os-release", "utf8");
    const match = release.match(/^PRETTY_NAME=(?:"([^"]+)"|([^\n]+))/m);
    const name = (match?.[1] || match?.[2] || "").trim();
    if (name) return name;
  } catch {
    /* ignore missing /etc/os-release */
  }
  return "Linux";
}
function selfTest(binary, cwd) {
  // No LD_LIBRARY_PATH: this must also work when launched outside a shell.
  const cleanEnv = { ...env };
  delete cleanEnv.LD_LIBRARY_PATH;
  execFileSync("dbus-run-session", ["--", "xvfb-run", "-a", binary, "--release-self-test"], { cwd, env: cleanEnv, stdio: "inherit", timeout: 180_000 });
}
try {
  const channels = preview ? ["development"] : process.argv.includes("--deb-only") ? ["deb"] : ["deb", "appimage"];
  const config = JSON.parse(readFileSync("apps/desktop/src-tauri/tauri.linux.release.conf.json", "utf8"));
  config.bundle.active = !preview;
  config.bundle.targets = preview ? ["deb"] : channels;
  config.bundle.createUpdaterArtifacts = channels.includes("appimage") && signed;
  if (signed) config.plugins = { updater: { pubkey: process.env.TAURI_UPDATER_PUBLIC_KEY, endpoints: ["https://github.com/Dreaminko/VRCS/releases/latest/download/latest.json"] } };
  writeFileSync(generated, JSON.stringify(config));
  tauri(["build", "--target", triple, "--features", "vulkan", "--config", "src-tauri/tauri.linux.generated.conf.json", "--no-bundle", "--no-sign", ...(preview ? ["--debug"] : []), "--", "--locked"]);
  const executable = join(binaryRoot, "vrcs-desktop");
  const rawExecutable = join(work, "vrcs-desktop.raw");
  copyFileSync(executable, rawExecutable);
  for (const channel of channels) {
    const stage = join(work, channel);
    const stageBin = join(stage, "usr/bin/vrcs-desktop");
    const stageLib = join(stage, "usr/lib/vrcs");
    mkdirSync(dirname(stageBin), { recursive: true });
    copyFileSync(rawExecutable, stageBin);
    const linked = collectRuntime(rawExecutable, stageLib);
    const bundledLibraries = readdirSync(stageLib).sort();
    verifyDirectDependencies(stageBin, bundledLibraries);
    run("patchelf", ["--set-rpath", "$ORIGIN/../lib/vrcs:$ORIGIN/../lib:$ORIGIN/lib", stageBin]);
    writeFileSync(join(artifactRoot, `linux-${arch}-${channel}.ldd.txt`), linked);
    const report = { os: hostOs(), arch, channel, host, systemPackages: DEB_DEPENDS, bundledLibraries, networkHelper: channel === "appimage" ? "requires system installation (/usr/bin/vrcs-desktop)" : "built-in (--network-helper)", onnx: "pending-self-test" };
    selfTest(stageBin, stage);
    report.onnx = "Silero-v6.2.1-session-loaded";
    writeFileSync(join(artifactRoot, `linux-${arch}-${channel}.runtime.json`), `${JSON.stringify(report, null, 2)}\n`);
    if (preview) {
      cpSync(stage, join(artifactRoot, `VRCS-preview-linux-${arch}`), { recursive: true });
      copyFileSync("LICENSE", join(artifactRoot, `VRCS-preview-linux-${arch}/LICENSE`));
      copyFileSync("THIRD_PARTY_NOTICES.md", join(artifactRoot, `VRCS-preview-linux-${arch}/THIRD_PARTY_NOTICES.md`));
      writeFileSync(join(artifactRoot, `VRCS-preview-linux-${arch}/README.txt`), "Unsigned development preview. Run usr/bin/vrcs-desktop with the entire directory intact. Requires the Linux system libraries listed in the adjacent runtime report and CJK fonts. This CI native ONNX self-test is not desktop/hardware acceptance or a published release. No automatic updates.\n");
      continue;
    }
    copyFileSync(stageBin, executable);
    const mapped = Object.fromEntries(readdirSync(stageLib).map((name) => [`/usr/lib/vrcs/${name}`, join(stageLib, name)]));
    config.bundle.linux.deb.files = {
      ...mapped,
      "/usr/share/polkit-1/actions/app.vrcs.network-capture.policy": resolve("packaging/linux/app.vrcs.network-capture.policy"),
    };
    config.bundle.linux.deb.depends = DEB_DEPENDS;
    config.bundle.linux.deb.postInstallScript = resolve("packaging/linux/postinst");
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
    if (channel === "appimage") selfTest(join(extracted, "squashfs-root", "AppRun"), join(extracted, "squashfs-root"));
    else selfTest(packagedBinary, dirname(packagedBinary));
  }
} finally {
  rmSync(generated, { force: true });
  rmSync(work, { recursive: true, force: true });
}
