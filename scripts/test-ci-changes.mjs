import assert from "node:assert/strict";
import { test } from "node:test";
import { classifyChanges } from "./ci-changes.mjs";

for (const [file, rust, frontend] of [
  ["README.md", false, false],
  ["Cargo.toml", true, false],
  ["Cargo.lock", true, false],
  ["core/src/lib.rs", true, false],
  ["crates/player-audio-scan/src/lib.rs", true, false],
  ["scripts/run-tauri.ps1", true, false],
  ["scripts/run-tauri.mjs", true, false],
  ["packaging/linux/app.vrcs.network-capture.policy", true, false],
  ["packaging/linux/postinst", true, false],
  ["scripts/dev-vr.mjs", false, true],
  ["scripts/dev-vr.test.mjs", false, true],
  ["apps/desktop/src/App.tsx", false, true],
  ["apps/desktop/src-tauri/src/lib.rs", true, true],
  ...["ci", "preview", "release", "nightly"].map((name) => [`.github/workflows/${name}.yml`, true, true]),
  ...["ci-changes.mjs", "test-ci-changes.mjs", "find-release-ci.mjs", "test-release-ci.mjs", "release-metadata.mjs", "test-release-metadata.mjs", "build-linux-release.mjs", "install-linux-deps.sh", "verify-linux-package.sh", "build-release.ps1", "write-release-metadata.ps1"].map((name) => [`scripts/${name}`, true, true]),
]) {
  test(`selects required checks for ${file}`, () => {
    assert.deepEqual(classifyChanges([file]), { rust, frontend });
  });
}
test("full release validation includes unchanged areas", () => {
  assert.deepEqual(classifyChanges(["README.md"], true), { rust: true, frontend: true });
});
test("independent changed areas accumulate", () => {
  assert.deepEqual(classifyChanges(["core/src/lib.rs", "apps/desktop/src/App.tsx"]), { rust: true, frontend: true });
});
