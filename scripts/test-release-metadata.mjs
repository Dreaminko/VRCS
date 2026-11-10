import assert from "node:assert/strict";
import { generateKeyPairSync, sign } from "node:crypto";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { aggregateRelease, releaseTargets, validatePackage, verifyUpdaterSignature, writeTargetManifest } from "./release-metadata.mjs";

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
function packageFixture(root, target) {
  const artifact = join(root, `VRCS-${version}-${target.suffix}`);
  if (target.channel === "deb") {
    const directory = join(root, `deb-${target.arch}`);
    mkdirSync(join(directory, "DEBIAN"), { recursive: true });
    mkdirSync(join(directory, "usr/bin"), { recursive: true });
    writeFileSync(join(directory, "DEBIAN/control"), `Package: vrcs\nVersion: ${version}\nArchitecture: ${target.arch === "x86_64" ? "amd64" : "arm64"}\nMaintainer: Test <test@example.invalid>\nDescription: test release consumer\n`);
    writeFileSync(join(directory, "usr/bin/vrcs-desktop"), elf(target.arch), { mode: 0o755 });
    execFileSync("dpkg-deb", ["--build", "--root-owner-group", directory, artifact], { stdio: "pipe" });
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
const linux = process.platform === "linux";
test("all published architectures aggregate without dropping Windows or its CUDA alias", { skip: !linux }, (t) => {
  const root = complete(t);
  const latest = JSON.parse(readFileSync(aggregateRelease({ root, version, publicKey: encodedKey }), "utf8"));
  assert.deepEqual(Object.keys(latest.platforms).sort(), ["windows-x86_64-standard", "windows-x86_64-cuda", "linux-x86_64-standard", "linux-aarch64-standard"].sort());
  assert.deepEqual(latest.platforms["windows-x86_64-cuda"], latest.platforms["windows-x86_64-standard"]);
  for (const arch of ["x86_64", "aarch64"]) assert.ok(latest.platforms[`linux-${arch}-standard`].url.endsWith(`linux-${arch}.AppImage`));
});
test("missing architecture never produces partial latest.json", { skip: !linux }, (t) => {
  const root = complete(t);
  rmSync(join(root, "linux-aarch64-appimage.manifest.json"));
  assert.throws(() => aggregateRelease({ root, version, publicKey: encodedKey }), /every release target/);
});
for (const mutation of ["empty signature", "wrong signature", "modified package", "wrong architecture"]) {
  test(`rejects ${mutation} before publishing updater metadata`, { skip: !linux }, (t) => {
    const root = complete(t);
    const artifact = join(root, `VRCS-${version}-linux-aarch64.AppImage`);
    if (mutation === "empty signature") writeFileSync(`${artifact}.sig`, " ");
    if (mutation === "wrong signature") writeFileSync(`${artifact}.sig`, signature(Buffer.from("different package")));
    if (mutation === "modified package") writeFileSync(artifact, Buffer.concat([readFileSync(artifact), Buffer.from("modified")]));
    if (mutation === "wrong architecture") writeFileSync(artifact, elf("x86_64", true));
    assert.throws(() => aggregateRelease({ root, version, publicKey: encodedKey }), /signature|checksum|architecture/i);
  });
}
test("deb control architecture cannot conceal a wrong ELF payload", { skip: !linux }, (t) => {
  const root = fixture(t);
  const target = releaseTargets.find((target) => target.channel === "deb" && target.arch === "aarch64");
  const artifact = packageFixture(root, target);
  const directory = join(root, "deb-aarch64");
  writeFileSync(join(directory, "usr/bin/vrcs-desktop"), elf("x86_64"));
  execFileSync("dpkg-deb", ["--build", directory, artifact], { stdio: "pipe" });
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
  const target = releaseTargets.find((target) => target.channel === "appimage" && target.arch === "aarch64");
  const artifact = join(root, "wrong-payload.AppImage");
  writeFileSync(artifact, appimage(root, "aarch64", "x86_64"));
  assert.throws(() => validatePackage(artifact, target), /ELF architecture/);
});

test("updater verifies both payload and trusted comment signatures", () => {
  const data = Buffer.from("signed package");
  assert.doesNotThrow(() => verifyUpdaterSignature(data, signature(data), encodedKey));
  const tampered = Buffer.from(signature(data), "base64").toString("utf8").replace("timestamp:12345", "timestamp:54321");
  assert.throws(() => verifyUpdaterSignature(data, Buffer.from(tampered).toString("base64"), encodedKey), /verification failed/);
});
