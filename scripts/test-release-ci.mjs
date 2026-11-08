import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { test } from "node:test";

const execute = promisify(execFile);
const script = fileURLToPath(new URL("./find-release-ci.mjs", import.meta.url));
const repository = "example/VRCS";
const sha = "a".repeat(40);
const requiredJobs = ["changes", "frontend", "rustfmt", "rust (cpu)", "rust (vulkan)", "gate"];
const successfulJobs = requiredJobs.map((name) => ({ name, status: "completed", conclusion: "success" }));
const successfulRun = {
  id: 123,
  run_attempt: 2,
  head_sha: sha,
  head_branch: "main",
  head_repository: { full_name: repository },
  path: ".github/workflows/ci.yml",
  event: "push",
  status: "completed",
  conclusion: "success",
};

async function lookup(t, { runs = [successfulRun], pages = [successfulJobs], errorAt, malformed = false } = {}) {
  const directory = await mkdtemp(join(tmpdir(), "vrcs-release-ci-"));
  const output = join(directory, "output");
  const requests = [];
  const server = createServer((request, response) => {
    const url = new URL(request.url, "http://localhost");
    requests.push(url);
    response.setHeader("Content-Type", "application/json");
    if (requests.length === errorAt) {
      response.writeHead(503).end('{}');
    } else if (malformed) {
      response.end("invalid JSON");
    } else if (url.pathname === `/repos/${repository}/actions/workflows/ci.yml/runs`) {
      response.end(JSON.stringify({ workflow_runs: runs }));
    } else if (url.pathname === `/repos/${repository}/actions/runs/123/attempts/2/jobs`) {
      response.end(JSON.stringify({
        total_count: pages.flat().length,
        jobs: pages[Number(url.searchParams.get("page") || 1) - 1] ?? [],
      }));
    } else {
      response.writeHead(404).end('{}');
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(async () => {
    await new Promise((resolve) => server.close(resolve));
    await rm(directory, { recursive: true, force: true });
  });
  const result = await execute(process.execPath, [script], {
    env: {
      ...process.env,
      GITHUB_REPOSITORY: repository,
      GITHUB_SHA: sha,
      GITHUB_API_URL: `http://127.0.0.1:${server.address().port}`,
      GH_TOKEN: "test-token",
      GITHUB_OUTPUT: output,
    },
  });
  return { output: await readFile(output, "utf8"), requests, ...result };
}

test("reuses a complete main CI run for the exact release commit and attempt", async (t) => {
  const result = await lookup(t);
  assert.equal(result.output, "reuse=true\n");
  assert.equal(result.requests[0].searchParams.get("head_sha"), sha);
  assert.equal(result.requests[0].searchParams.get("branch"), "main");
  assert.equal(result.requests[0].searchParams.get("event"), "push");
  assert.match(result.requests[1].pathname, /\/attempts\/2\/jobs$/);
});

test("reads every page of the selected CI attempt", async (t) => {
  const otherJobs = Array.from({ length: 100 }, (_, index) => ({ name: `optional-${index}`, conclusion: "success" }));
  const result = await lookup(t, { pages: [otherJobs, successfulJobs] });
  assert.equal(result.output, "reuse=true\n");
  assert.equal(result.requests.length, 3);
  assert.equal(result.requests[2].searchParams.get("page"), "2");
});

test("falls back when no CI run exists", async (t) => {
  assert.equal((await lookup(t, { runs: [] })).output, "reuse=false\n");
});

for (const [name, changed] of [
  ["another commit", { head_sha: "b".repeat(40) }],
  ["another branch", { head_branch: "feature" }],
  ["a fork", { head_repository: { full_name: "fork/VRCS" } }],
  ["a pull request", { event: "pull_request" }],
  ["another workflow", { path: ".github/workflows/preview.yml" }],
  ["an unfinished run", { status: "in_progress", conclusion: null }],
  ["a failed run", { conclusion: "failure" }],
  ["a cancelled run", { conclusion: "cancelled" }],
]) {
  test(`rejects ${name}`, async (t) => {
    const result = await lookup(t, { runs: [{ ...successfulRun, ...changed }] });
    assert.equal(result.output, "reuse=false\n");
    assert.equal(result.requests.length, 1);
  });
}

test("does not select an older success over a newer failed run", async (t) => {
  const result = await lookup(t, { runs: [{ ...successfulRun, conclusion: "failure" }, successfulRun] });
  assert.equal(result.output, "reuse=false\n");
});

for (const name of requiredJobs) {
  test(`requires successful ${name}`, async (t) => {
    const jobs = successfulJobs.map((job) => job.name === name ? { ...job, conclusion: "skipped" } : job);
    assert.equal((await lookup(t, { pages: [jobs] })).output, "reuse=false\n");
  });
}

test("rejects an incomplete job list", async (t) => {
  assert.equal((await lookup(t, { pages: [successfulJobs.slice(1)] })).output, "reuse=false\n");
});

for (const errorAt of [1, 2]) {
  test(`falls back when API request ${errorAt} fails`, async (t) => {
    assert.equal((await lookup(t, { errorAt })).output, "reuse=false\n");
  });
}

test("falls back on malformed API data", async (t) => {
  assert.equal((await lookup(t, { malformed: true })).output, "reuse=false\n");
});
