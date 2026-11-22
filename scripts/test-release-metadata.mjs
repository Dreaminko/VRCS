import assert from "node:assert/strict";
import { generateKeyPairSync, sign } from "node:crypto";
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import { test } from "node:test";
import { aggregateRelease, checkProjectVersion, releaseTargets, validatePackage, verifyUpdaterSignature, writeTargetManifest } from "./release-metadata.mjs";

const version = "1.2.3";
const { publicKey, privateKey } = generateKeyPairSync("ed25519");
const rawKey = publicKey.export({ format: "der", type: "spki" }).subarray(-32);
const keyId = Buffer.from("0102030405060708", "hex");
const encodedKey = Buffer.from(`untrusted comment: test key\n${Buffer.concat([Buffer.from("Ed"), keyId, rawKey]).toString("base64")}\n`).toString("base64");
function signature(data) {
  const sig = sign(null, data, privateKey);
  const packet = Buffer.concat([Buffer.from("Ed"), keyId, sig]);
  const comment = "timestamp:12345";
  const global = sign(null, Buffer.concat([sig, Buffer.from(comment)]), privateKey);
  return Buffer.from(`untrusted comment: test signature\n${packet.toString("base64")}\ntrusted comment: ${comment}\n${global.toString("base64")}\n`).toString("base64");
}
function elf(arch, appimage = false) {
  const data = Buffer.alloc(64);
  Buffer.from([127, 69, 76, 70, 2, 1]).copy(data);
  data.writeUInt16LE(arch === "x86_64" ? 62 : 183, 18);
  if (appimage) Buffer.from([65, 73, 2]).copy(data, 8);
  return data;
}
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "vrcs-metadata-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}
function appimage(root, arch, payloadArch = arch) {
  const directory = join(root, `appdir-${arch}-${payloadArch}`);
  mkdirSync(join(directory, "usr/bin"), { recursive: true });
  writeFileSync(join(directory, "usr/bin/vrcs-desktop"), elf(payloadArch), { mode: 0o755 });
  const squashfs = join(root, `payload-${arch}-${payloadArch}.squashfs`);
  execFileSync("mksquashfs", [directory, squashfs, "-noappend", "-no-progress", "-processors", "1"], { stdio: "pipe" });
  return Buffer.concat([elf(arch, true), readFileSync(squashfs)]);
}
function buildRpmFixture(root, arch, artifact, payloadArch = arch) {
  const rpmTop = join(root, `rpm-${arch}-${payloadArch}`);
  for (const dir of ["SPECS", "BUILD", "BUILDROOT", "RPMS", "SRPMS", "SOURCES"]) {
    mkdirSync(join(rpmTop, dir), { recursive: true });
  }
  const stageRoot = join(rpmTop, "STAGE");
  mkdirSync(join(stageRoot, "usr/bin"), { recursive: true });
  mkdirSync(join(stageRoot, "usr/lib/vrcs"), { recursive: true });
  writeFileSync(join(stageRoot, "usr/bin/vrcs-desktop"), elf(payloadArch), { mode: 0o755 });
  const specPath = join(rpmTop, "SPECS/vrcs.spec");
  writeFileSync(specPath, [
    `%define _topdir ${rpmTop}`,
    "%define _build_id_links none",
    "%define __strip /bin/true",
    "%define __os_install_post %{nil}",
    "%define debug_package %{nil}",
    "Name: vrcs",
    `Version: ${version}`,
    "Release: 1",
    "Summary: test release consumer",
    "License: AGPL-3.0-only",
    `BuildArch: ${arch}`,
    "AutoReqProv: no",
    "%description",
    "test release consumer",
    "%install",
    "mkdir -p %{buildroot}/usr/bin %{buildroot}/usr/lib/vrcs",
    `cp -a ${stageRoot}/usr/bin/vrcs-desktop %{buildroot}/usr/bin/vrcs-desktop`,
    "%files",
    "%attr(0755, root, root) /usr/bin/vrcs-desktop",
    "%dir /usr/lib/vrcs",
    "",
  ].join("\n"));
  execFileSync("rpmbuild", ["-bb", "--nodeps", "--define", `_topdir ${rpmTop}`, specPath], { stdio: "pipe" });
  copyFileSync(join(rpmTop, "RPMS", arch, `vrcs-${version}-1.${arch}.rpm`), artifact);
}
function packageFixture(root, target) {
  const artifact = join(root, `VRCS-${version}-${target.suffix}`);
  if (target.channel === "deb") {
    const directory = join(root, `deb-${target.arch}`);
    mkdirSync(join(directory, "DEBIAN"), { recursive: true });
    mkdirSync(join(directory, "usr/bin"), { recursive: true });
    mkdirSync(join(directory, "usr/lib/vrcs"), { recursive: true });
    writeFileSync(join(directory, "DEBIAN/control"), `Package: vrcs\nVersion: ${version}\nArchitecture: ${target.arch === "x86_64" ? "amd64" : "arm64"}\nMaintainer: Test <test@example.invalid>\nDescription: test release consumer\n`);
    writeFileSync(join(directory, "usr/bin/vrcs-desktop"), elf(target.arch), { mode: 0o755 });
    execFileSync("dpkg-deb", ["--build", "--root-owner-group", directory, artifact], { stdio: "pipe" });
  } else if (target.channel === "rpm") {
    buildRpmFixture(root, target.arch, artifact);
  } else if (target.channel === "appimage") writeFileSync(artifact, appimage(root, target.arch));
  else {
    const data = Buffer.alloc(128);
    data.write("MZ");
    data.writeUInt32LE(64, 60);
    data.write("PE\0\0", 64);
    data.writeUInt16LE(0x14c, 68);
    writeFileSync(artifact, data);
  }
  if (target.targets.length) writeFileSync(`${artifact}.sig`, signature(readFileSync(artifact)));
  writeTargetManifest({ root, version, ...target });
  return artifact;
}
function complete(t) {
  const root = fixture(t);
  for (const target of releaseTargets) packageFixture(root, target);
  return root;
}
test("every release manifest must agree on the version", (t) => {
  const root = fixture(t);
  const tauri = join(root, "apps/desktop/src-tauri");
  mkdirSync(tauri, { recursive: true });
  mkdirSync(join(root, "core"), { recursive: true });
  writeFileSync(join(tauri, "tauri.conf.json"), JSON.stringify({ version }));
  writeFileSync(join(tauri, "Cargo.toml"), `[package]\nversion = "${version}"\n`);
  writeFileSync(join(root, "core/Cargo.toml"), `[package]\nversion = "${version}"\n`);
  writeFileSync(join(root, "apps/desktop/package.json"), JSON.stringify({ version }));
  assert.doesNotThrow(() => checkProjectVersion(version, root));
  writeFileSync(join(root, "apps/desktop/package.json"), JSON.stringify({ version: "9.9.9" }));
  assert.throws(() => checkProjectVersion(version, root), /version mismatch/);
});

const linux = process.platform === "linux";
test("all published architectures aggregate cleanly with Windows and Linux standard targets", { skip: !linux }, (t) => {
  const root = complete(t);
  const latest = JSON.parse(readFileSync(aggregateRelease({ root, version, publicKey: encodedKey }), "utf8"));
  assert.deepEqual(Object.keys(latest.platforms).sort(), ["windows-x86_64-standard", "linux-x86_64-standard"].sort());
  for (const arch of ["x86_64"]) assert.ok(latest.platforms[`linux-${arch}-standard`].url.endsWith(`linux-${arch}.AppImage`));
});
test("missing architecture never produces partial latest.json", { skip: !linux }, (t) => {
  const root = complete(t);
  rmSync(join(root, "linux-x86_64-appimage.manifest.json"));
  assert.throws(() => aggregateRelease({ root, version, publicKey: encodedKey }), /every release target/);
});
test("missing rpm manifest never produces partial latest.json", { skip: !linux }, (t) => {
  const root = complete(t);
  rmSync(join(root, "linux-x86_64-rpm.manifest.json"));
  assert.throws(() => aggregateRelease({ root, version, publicKey: encodedKey }), /every release target/);
});
test("rejects fabricated updater signature on deb or rpm packages", { skip: !linux }, (t) => {
  for (const channel of ["deb", "rpm"]) {
    const root = complete(t);
    const manifestPath = join(root, `linux-x86_64-${channel}.manifest.json`);
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    manifest.signature = `${manifest.artifact}.sig`;
    writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
    writeFileSync(join(root, manifest.signature), signature(readFileSync(join(root, manifest.artifact))));
    assert.throws(() => aggregateRelease({ root, version, publicKey: encodedKey }), /package-manager updates/);
  }
});
for (const mutation of ["empty signature", "wrong signature", "modified package", "wrong architecture"]) {
  test(`rejects ${mutation} before publishing updater metadata`, { skip: !linux }, (t) => {
    const root = complete(t);
    const artifact = join(root, `VRCS-${version}-linux-x86_64.AppImage`);
    if (mutation === "empty signature") writeFileSync(`${artifact}.sig`, " ");
    if (mutation === "wrong signature") writeFileSync(`${artifact}.sig`, signature(Buffer.from("different package")));
    if (mutation === "modified package") writeFileSync(artifact, Buffer.concat([readFileSync(artifact), Buffer.from("modified")]));
    if (mutation === "wrong architecture") writeFileSync(artifact, elf("aarch64", true));
    assert.throws(() => aggregateRelease({ root, version, publicKey: encodedKey }), /signature|checksum|architecture/i);
  });
}
test("deb control architecture cannot conceal a wrong ELF payload", { skip: !linux }, (t) => {
  const root = fixture(t);
  const target = releaseTargets.find((target) => target.channel === "deb" && target.arch === "x86_64");
  const artifact = packageFixture(root, target);
  const directory = join(root, "deb-x86_64");
  writeFileSync(join(directory, "usr/bin/vrcs-desktop"), elf("aarch64"));
  execFileSync("dpkg-deb", ["--build", directory, artifact], { stdio: "pipe" });
  assert.throws(() => validatePackage(artifact, target), /ELF architecture/);
});
test("rpm header architecture cannot conceal a wrong ELF payload", { skip: !linux }, (t) => {
  const root = fixture(t);
  const target = releaseTargets.find((target) => target.channel === "rpm" && target.arch === "x86_64");
  const artifact = join(root, `VRCS-${version}-${target.suffix}`);
  buildRpmFixture(root, "x86_64", artifact, "aarch64");
  assert.throws(() => validatePackage(artifact, target), /ELF architecture/);
});
test("unsigned local AppImages have no fabricated signature", { skip: !linux }, (t) => {
  const root = fixture(t);
  const target = releaseTargets.find((target) => target.channel === "appimage");
  writeFileSync(join(root, `VRCS-${version}-${target.suffix}`), appimage(root, target.arch));
  const manifest = JSON.parse(readFileSync(writeTargetManifest({ root, version, ...target }), "utf8"));
  assert.equal(manifest.signature, null);
});
test("AppImage runtime architecture cannot conceal a wrong payload", { skip: !linux }, (t) => {
  const root = fixture(t);
  const target = releaseTargets.find((target) => target.channel === "appimage" && target.arch === "x86_64");
  const artifact = join(root, "wrong-payload.AppImage");
  writeFileSync(artifact, appimage(root, "x86_64", "aarch64"));
  assert.throws(() => validatePackage(artifact, target), /ELF architecture/);
});

test("validatePackage and writeTargetManifest preserve extractTo for deb and rpm", { skip: !linux }, (t) => {
  const root = fixture(t);
  for (const channel of ["deb", "rpm"]) {
    const target = releaseTargets.find((item) => item.channel === channel && item.arch === "x86_64");
    const artifact = packageFixture(root, target);
    const validateDir = join(root, `extract-validate-${channel}`);
    validatePackage(artifact, target, { extractTo: validateDir });
    assert.ok(existsSync(join(validateDir, "usr/bin/vrcs-desktop")));

    const manifestDir = join(root, `extract-manifest-${channel}`);
    const manifestPath = writeTargetManifest({ root, version, ...target, extractTo: manifestDir });
    assert.ok(existsSync(manifestPath));
    assert.ok(existsSync(join(manifestDir, "usr/bin/vrcs-desktop")));
  }
});

test("aggregateRelease accepts relative artifact directories including RPM packages", { skip: !linux }, (t) => {
  const root = complete(t);
  const relativeRoot = relative(process.cwd(), root);
  const latestPath = aggregateRelease({ root: relativeRoot, version, publicKey: encodedKey });
  assert.ok(existsSync(latestPath));
});

test("validatePackage checks PE headers without loading invalid installers", (t) => {
  const root = fixture(t);
  const target = releaseTargets.find((item) => item.channel === "nsis");
  const artifact = packageFixture(root, target);
  assert.doesNotThrow(() => validatePackage(artifact, target));

  const empty = join(root, "empty.exe");
  writeFileSync(empty, Buffer.alloc(0));
  assert.throws(() => validatePackage(empty, target), /empty/);

  const badOffset = join(root, "bad-offset.exe");
  const data = Buffer.alloc(128);
  data.write("MZ");
  data.writeUInt32LE(512, 60);
  writeFileSync(badOffset, data);
  assert.throws(() => validatePackage(badOffset, target), /Invalid PE/);
});

test("updater verifies both payload and trusted comment signatures", () => {
  const data = Buffer.from("signed package");
  assert.doesNotThrow(() => verifyUpdaterSignature(data, signature(data), encodedKey));
  const tampered = Buffer.from(signature(data), "base64").toString("utf8").replace("timestamp:12345", "timestamp:54321");
  assert.throws(() => verifyUpdaterSignature(data, Buffer.from(tampered).toString("base64"), encodedKey), /verification failed/);
});
