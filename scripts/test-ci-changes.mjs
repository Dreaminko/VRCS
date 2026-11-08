import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

const workflow = readFileSync(new URL("../.github/workflows/ci.yml", import.meta.url), "utf8");
const filter = workflow.match(/        id: filter\r?\n[\s\S]*?        run: \|\r?\n((?:          [^\r\n]*\r?\n|\r?\n)+)/);
assert.ok(filter, "The changed-area detection step must be present");
const script = filter[1].replace(/\r\n/g, "\n").replace(/^ {10}/gm, "");

function git(cwd, ...args) {
  const result = spawnSync("git", args, { cwd, encoding: "utf8" });
  assert.equal(result.status, 0, result.error?.message ?? result.stderr);
  return result.stdout.trim();
}

function detect({ file = "README.md", base, full = false }) {
  const root = mkdtempSync(join(tmpdir(), "vrcs-ci-changes-"));
  try {
    const repo = join(root, "repository");
    mkdirSync(repo);
    git(repo, "init", "--quiet");
    writeFileSync(join(repo, "README.md"), "fixture\n");
    git(repo, "add", ".");
    git(repo, "-c", "user.name=CI Test", "-c", "user.email=ci@example.invalid", "commit", "--quiet", "-m", "base");
    const baseSha = git(repo, "rev-parse", "HEAD");
    mkdirSync(dirname(join(repo, file)), { recursive: true });
    writeFileSync(join(repo, file), "changed\n");
    git(repo, "add", ".");
    git(repo, "-c", "user.name=CI Test", "-c", "user.email=ci@example.invalid", "commit", "--quiet", "-m", "change");

    const output = join(root, "output");
    writeFileSync(output, "");
    const result = spawnSync(process.env.BASH || "bash", ["--noprofile", "--norc", "-e", "-o", "pipefail", "-c", script], {
      cwd: repo,
      encoding: "utf8",
      env: { ...process.env, BASE_SHA: base ?? baseSha, FULL_CHECK: String(full), GITHUB_OUTPUT: output },
    });
    assert.ifError(result.error);
    return { status: result.status, stderr: result.stderr, output: readFileSync(output, "utf8") };
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

for (const [file, rust, frontend] of [
  ["README.md", false, false],
  ["core/src/lib.rs", true, false],
  ["crates/player-audio-scan/src/lib.rs", true, false],
  ["apps/desktop/src/App.tsx", false, true],
  ["apps/desktop/src-tauri/src/lib.rs", true, true],
  [".github/workflows/ci.yml", true, true],
  ["scripts/test-ci-changes.mjs", true, true],
  ["scripts/find-release-ci.mjs", true, true],
  ["scripts/test-release-ci.mjs", true, true],
]) {
  test(`selects checks for ${file}`, () => {
    const result = detect({ file });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.output, `rust=${rust}\nfrontend=${frontend}\n`);
  });
}

for (const options of [{ base: "" }, { base: "0".repeat(40) }, { full: true }, { full: true, base: "missing-ref" }]) {
  test(`runs all checks for ${JSON.stringify(options)}`, () => {
    const result = detect(options);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.output, "rust=true\nfrontend=true\n");
  });
}

test("fails when the comparison base cannot be resolved", () => {
  const result = detect({ base: "missing-ref" });
  assert.notEqual(result.status, 0);
  assert.equal(result.output, "");
});
