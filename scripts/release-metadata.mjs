import { createHash, createPublicKey, verify } from "node:crypto";
import { closeSync, copyFileSync, existsSync, linkSync, mkdirSync, mkdtempSync, openSync, readFileSync, readSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, isAbsolute, join, relative, resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const versionPattern = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;
export const releaseTargets = [
  { os: "windows", arch: "x86_64", channel: "nsis", suffix: "windows-x64.exe", targets: ["windows-x86_64-standard"] },
  ...["x86_64"].flatMap((arch) => [
    { os: "linux", arch, channel: "deb", suffix: `linux-${arch}.deb`, targets: [] },
    { os: "linux", arch, channel: "rpm", suffix: `linux-${arch}.rpm`, targets: [] },
    { os: "linux", arch, channel: "appimage", suffix: `linux-${arch}.AppImage`, targets: [`linux-${arch}-standard`] },
  ]),
];
const digest = (data) => createHash("sha256").update(data).digest("hex");
function requireVersion(version) {
  if (!versionPattern.test(version)) throw new Error("Invalid release version");
}
const tomlVersion = (data) => data.match(/^version = "([^"]+)"/m)?.[1];

/// Every release manifest must carry the same version before anything is built.
export function checkProjectVersion(version, root = ".") {
  requireVersion(version);
  const manifests = [
    ["apps/desktop/src-tauri/tauri.conf.json", (data) => JSON.parse(data).version],
    ["core/Cargo.toml", tomlVersion],
    ["apps/desktop/src-tauri/Cargo.toml", tomlVersion],
    ["apps/desktop/package.json", (data) => JSON.parse(data).version],
  ];
  for (const [path, read] of manifests) {
    if (read(readFileSync(join(root, path), "utf8")) !== version) {
      throw new Error(`Release version mismatch: ${path}`);
    }
  }
}

function targetFor(os, arch, channel) {
  const target = releaseTargets.find((item) => item.os === os && item.arch === arch && item.channel === channel);
  if (!target) throw new Error(`Unsupported release target: ${os}/${arch}/${channel}`);
  return target;
}
function readHead(path, length, position = 0) {
  const fd = openSync(path, "r");
  try {
    const buffer = Buffer.allocUnsafe(length);
    let total = 0;
    while (total < length) {
      const bytes = readSync(fd, buffer, total, length - total, position + total);
      if (bytes === 0) break;
      total += bytes;
    }
    return buffer.subarray(0, total);
  } finally {
    closeSync(fd);
  }
}
function elfArchitecture(data, arch) {
  const machine = { x86_64: 62, aarch64: 183 }[arch];
  if (data.length < 64 || !data.subarray(0, 4).equals(Buffer.from([127, 69, 76, 70])) || data[4] !== 2 || data[5] !== 1 || data.readUInt16LE(18) !== machine) {
    throw new Error(`ELF architecture does not match ${arch}`);
  }
}
function elfArchitectureFile(filePath, arch) {
  elfArchitecture(readHead(filePath, 64), arch);
}
function walk(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((item) => {
    const path = join(directory, item.name);
    return item.isDirectory() ? walk(path) : item.isFile() ? [path] : [];
  });
}
function validateExtractedLinuxTree(directory, arch, label) {
  const binDir = join(directory, "usr", "bin");
  if (!existsSync(binDir)) throw new Error(`${label} must contain vrcs-desktop`);
  const executables = walk(binDir).filter((file) => basename(file) === "vrcs-desktop");
  if (executables.length !== 1) throw new Error(`${label} must contain vrcs-desktop`);
  elfArchitectureFile(executables[0], arch);
  for (const file of walk(directory).filter((file) => /\.so(?:\.|$)/.test(file))) {
    elfArchitectureFile(file, arch);
  }
}
export function validatePackage(path, target, options = {}) {
  const packagePath = resolve(path);
  const { size } = statSync(packagePath);
  if (!size) throw new Error("Release package is empty");
  if (target.channel === "appimage") {
    const data = readFileSync(packagePath);
    elfArchitecture(data, target.arch);
    if (!data.subarray(8, 11).equals(Buffer.from([65, 73, 2]))) throw new Error("Expected a type-2 AppImage");
    const directory = mkdtempSync(join(tmpdir(), "vrcs-appimage-"));
    try {
      let offset = -1;
      for (let candidate = data.indexOf("hsqs"); candidate >= 0; candidate = data.indexOf("hsqs", candidate + 4)) {
        if (candidate + 96 <= data.length && data.readUInt16LE(candidate + 28) === 4 && data.readUInt16LE(candidate + 30) === 0 && data.readBigUInt64LE(candidate + 40) <= BigInt(data.length - candidate)) { offset = candidate; break; }
      }
      if (offset < 0) throw new Error("AppImage SquashFS payload is missing");
      const extracted = join(directory, "AppDir");
      execFileSync("unsquashfs", ["-no-progress", "-o", String(offset), "-d", extracted, packagePath], { stdio: "pipe" });
      elfArchitectureFile(join(extracted, "usr", "bin", "vrcs-desktop"), target.arch);
      for (const file of walk(extracted).filter((file) => /\.so(?:\.|$)/.test(file))) elfArchitectureFile(file, target.arch);
    } finally { rmSync(directory, { recursive: true, force: true }); }
  } else if (target.channel === "deb") {
    const expected = target.arch === "x86_64" ? "amd64" : "arm64";
    const actual = execFileSync("dpkg-deb", ["--field", packagePath, "Architecture"], { encoding: "utf8" }).trim();
    if (actual !== expected) throw new Error(`Deb architecture ${actual} does not match ${expected}`);
    const directory = options.extractTo ?? mkdtempSync(join(tmpdir(), "vrcs-deb-"));
    if (options.extractTo) mkdirSync(directory, { recursive: true });
    try {
      execFileSync("dpkg-deb", ["--extract", packagePath, directory]);
      validateExtractedLinuxTree(directory, target.arch, "Deb");
    } finally { if (!options.extractTo) rmSync(directory, { recursive: true, force: true }); }
  } else if (target.channel === "rpm") {
    const header = readHead(packagePath, 96);
    if (size < 96 || header.length < 96 || !header.subarray(0, 4).equals(Buffer.from([0xed, 0xab, 0xee, 0xdb]))) {
      throw new Error("Expected an RPM package");
    }
    const actual = execFileSync("rpm", ["-qp", "--nosignature", "--qf", "%{ARCH}\n", packagePath], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    }).trim();
    if (actual !== target.arch) throw new Error(`Rpm architecture ${actual} does not match ${target.arch}`);
    const directory = options.extractTo ?? mkdtempSync(join(tmpdir(), "vrcs-rpm-"));
    if (options.extractTo) mkdirSync(directory, { recursive: true });
    try {
      execFileSync("sh", ["-c", 'rpm2cpio "$1" | cpio -idm --no-absolute-filenames --quiet', "--", packagePath], {
        cwd: directory,
        stdio: "pipe",
      });
      validateExtractedLinuxTree(directory, target.arch, "Rpm");
    } finally { if (!options.extractTo) rmSync(directory, { recursive: true, force: true }); }
  } else {
    const dosHeader = readHead(packagePath, 64);
    if (dosHeader.length < 64 || dosHeader.toString("ascii", 0, 2) !== "MZ") throw new Error("Expected a PE installer");
    const offset = dosHeader.readUInt32LE(60);
    const peHeader = offset + 6 <= size ? readHead(packagePath, 6, offset) : Buffer.alloc(0);
    // NSIS uses an x86 bootstrapper even when its installed application is x64.
    if (offset + 6 > size || peHeader.length < 6 || peHeader.toString("ascii", 0, 4) !== "PE\0\0" || ![0x14c, 0x8664].includes(peHeader.readUInt16LE(4))) throw new Error("Invalid PE installer");
  }
}
export function verifyUpdaterSignature(data, signature, encodedPublicKey) {
  if (!signature?.trim()) throw new Error("Updater signature must not be empty");
  if (!encodedPublicKey?.trim()) throw new Error("Updater public key is required");
  const keyLines = Buffer.from(encodedPublicKey.trim(), "base64").toString("utf8").trim().split(/\r?\n/);
  const lines = Buffer.from(signature.trim(), "base64").toString("utf8").trim().split(/\r?\n/);
  const key = Buffer.from(keyLines[1] || "", "base64");
  const packet = Buffer.from(lines[1] || "", "base64");
  const globalSignature = Buffer.from(lines[3] || "", "base64");
  if (key.length !== 42 || packet.length !== 74 || globalSignature.length !== 64 || !lines[2]?.startsWith("trusted comment: ") || !key.subarray(2, 10).equals(packet.subarray(2, 10))) throw new Error("Malformed updater signature or mismatched signing key");
  const algorithm = packet.toString("ascii", 0, 2);
  if (key.toString("ascii", 0, 2) !== "Ed" || !["Ed", "ED"].includes(algorithm)) throw new Error("Unsupported updater signature algorithm");
  const publicKey = createPublicKey({ key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), key.subarray(10)]), format: "der", type: "spki" });
  const signed = algorithm === "ED" ? createHash("blake2b512").update(data).digest() : data;
  if (!verify(null, signed, publicKey, packet.subarray(10)) || !verify(null, Buffer.concat([packet.subarray(10), Buffer.from(lines[2].slice("trusted comment: ".length))]), publicKey, globalSignature)) throw new Error("Updater signature verification failed");
}
export function writeTargetManifest({ root, version, os, arch, channel, extractTo }) {
  requireVersion(version);
  const target = targetFor(os, arch, channel);
  const artifact = `VRCS-${version}-${target.suffix}`;
  const path = join(root, artifact);
  validatePackage(path, target, extractTo ? { extractTo } : {});
  const sha256 = digest(readFileSync(path));
  writeFileSync(`${path}.sha256`, `${sha256}  ${artifact}\n`);
  const signaturePath = `${path}.sig`;
  let signature = null;
  if (target.targets.length) {
    try { signature = readFileSync(signaturePath, "utf8").trim(); }
    catch (error) { if (error.code !== "ENOENT") throw error; }
    if (signature !== null && !signature) throw new Error("Updater signature must not be empty");
  }
  const manifest = { version, os, arch, channel, artifact, sha256, size: statSync(path).size, signature: signature ? `${artifact}.sig` : null };
  const manifestPath = join(root, `${os}-${arch}-${channel}.manifest.json`);
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  return manifestPath;
}
function pathsOverlap(a, b) {
  const relAB = relative(a, b);
  const relBA = relative(b, a);
  return relAB === "" || (!relAB.startsWith("..") && !isAbsolute(relAB)) || (!relBA.startsWith("..") && !isAbsolute(relBA));
}
export function publishAssetNames(version) {
  requireVersion(version);
  return [
    ...releaseTargets.map((target) => `VRCS-${version}-${target.suffix}`),
    "SHA256SUMS.txt",
    "latest.json",
  ];
}
export function stagePublishAssets({ root, version, publishDir }) {
  requireVersion(version);
  const resolvedRoot = resolve(root);
  const resolvedPublish = resolve(publishDir);
  if (pathsOverlap(resolvedRoot, resolvedPublish)) {
    throw new Error("Publish directory must be separate from artifact root");
  }
  const names = publishAssetNames(version);
  for (const name of names) {
    if (!existsSync(join(resolvedRoot, name))) {
      throw new Error(`Missing publishable release asset: ${name}`);
    }
  }
  rmSync(resolvedPublish, { recursive: true, force: true });
  mkdirSync(resolvedPublish, { recursive: true });
  return names.map((name) => {
    const source = join(resolvedRoot, name);
    const destination = join(resolvedPublish, name);
    try {
      linkSync(source, destination);
    } catch (error) {
      if (error.code === "EXDEV" || error.code === "EPERM") copyFileSync(source, destination);
      else throw error;
    }
    return destination;
  });
}
export function aggregateRelease({ root, version, publicKey, repository = "Dreaminko/VRCS", now = new Date(), publishDir }) {
  requireVersion(version);
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository)) throw new Error("Invalid release repository");
  const platforms = {};
  const checksums = [];
  const manifestFiles = readdirSync(root).filter((file) => file.endsWith(".manifest.json"));
  if (manifestFiles.length !== releaseTargets.length) throw new Error("Expected every release target manifest exactly once");
  for (const target of releaseTargets) {
    const manifest = JSON.parse(readFileSync(join(root, `${target.os}-${target.arch}-${target.channel}.manifest.json`), "utf8"));
    const artifact = `VRCS-${version}-${target.suffix}`;
    if (manifest.version !== version || manifest.os !== target.os || manifest.arch !== target.arch || manifest.channel !== target.channel || manifest.artifact !== artifact) throw new Error("Release target manifest mismatch");
    const path = join(root, artifact);
    validatePackage(path, target);
    const data = readFileSync(path);
    const hash = digest(data);
    if (manifest.size !== data.length || manifest.sha256 !== hash || readFileSync(`${path}.sha256`, "utf8").trim() !== `${hash}  ${artifact}`) throw new Error("Release checksum mismatch");
    if (target.targets.length) {
      if (manifest.signature !== `${artifact}.sig`) throw new Error("Signed updater package required");
      const signature = readFileSync(`${path}.sig`, "utf8").trim();
      verifyUpdaterSignature(data, signature, publicKey);
      for (const name of target.targets) platforms[name] = { url: `https://github.com/${repository}/releases/download/${version}/${artifact}`, signature };
    } else if (manifest.signature !== null) throw new Error("System packages (deb/rpm) must use package-manager updates");
    checksums.push({ artifact, hash });
  }
  checksums.sort((a, b) => (a.artifact < b.artifact ? -1 : a.artifact > b.artifact ? 1 : 0));
  writeFileSync(join(root, "SHA256SUMS.txt"), `${checksums.map(({ artifact, hash }) => `${hash}  ${artifact}`).join("\n")}\n`);
  const latest = { version, notes: `VRCS ${version}. Windows x64 and Linux x86_64 (deb, rpm, AppImage; Ubuntu 24.04 / glibc 2.39+ baseline). System packages (deb/rpm) update through the package manager; signed AppImage packages support in-app updates. Hardware compatibility is documented separately.`, pub_date: now.toISOString(), platforms };
  const path = join(root, "latest.json");
  writeFileSync(path, `${JSON.stringify(latest, null, 2)}\n`);
  if (publishDir) stagePublishAssets({ root, version, publishDir });
  return path;
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, ...args] = process.argv.slice(2);
  if (command === "target") console.log(writeTargetManifest({ root: args[0], version: args[1], os: args[2], arch: args[3], channel: args[4] }));
  else if (command === "aggregate") console.log(aggregateRelease({ root: args[0], version: args[1], publicKey: process.env.TAURI_UPDATER_PUBLIC_KEY, repository: process.env.GITHUB_REPOSITORY || "Dreaminko/VRCS", publishDir: args[2] }));
  else if (command === "check-version") checkProjectVersion(args[0], args[1] || ".");
  else throw new Error("Usage: release-metadata.mjs target ROOT VERSION OS ARCH CHANNEL | aggregate ROOT VERSION [PUBLISH_DIR] | check-version VERSION [ROOT]");
}
