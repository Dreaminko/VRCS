import { appendFileSync } from "node:fs";

const repository = process.env.GITHUB_REPOSITORY;
const sha = process.env.GITHUB_SHA;
const api = `${process.env.GITHUB_API_URL || "https://api.github.com"}/repos/${repository}`;
const requiredJobs = ["changes", "frontend", "rustfmt", "rust (cpu)", "rust (vulkan)", "linux (x86_64)", "linux (aarch64)", "gate"];

async function request(path) {
  const response = await fetch(`${api}${path}`, {
    headers: {
      Accept: "application/vnd.github+json",
      ...(process.env.GH_TOKEN ? { Authorization: `Bearer ${process.env.GH_TOKEN}` } : {}),
    },
    signal: AbortSignal.timeout(10_000),
  });
  if (!response.ok) throw new Error(`GitHub API returned ${response.status}`);
  return response.json();
}

async function findRun() {
  const query = new URLSearchParams({ head_sha: sha, branch: "main", event: "push", per_page: "1" });
  const { workflow_runs: runs } = await request(`/actions/workflows/ci.yml/runs?${query}`);
  const run = runs[0];
  if (
    !run || run.head_sha !== sha || run.head_branch !== "main" || run.event !== "push" ||
    run.head_repository?.full_name?.toLowerCase() !== repository.toLowerCase() ||
    run.path !== ".github/workflows/ci.yml" || run.status !== "completed" || run.conclusion !== "success" ||
    !Number.isSafeInteger(run.id) || !Number.isSafeInteger(run.run_attempt)
  ) return null;

  const jobs = [];
  for (let page = 1; ; page++) {
    const result = await request(`/actions/runs/${run.id}/attempts/${run.run_attempt}/jobs?per_page=100&page=${page}`);
    if (!Array.isArray(result.jobs) || !result.jobs.length || !Number.isSafeInteger(result.total_count)) return null;
    jobs.push(...result.jobs);
    if (jobs.length >= result.total_count) break;
  }
  return requiredJobs.every((name) => {
    const matches = jobs.filter((job) => job.name === name);
    return matches.length === 1 && matches[0].status === "completed" && matches[0].conclusion === "success";
  }) ? run : null;
}

let run;
try {
  run = await findRun();
} catch (error) {
  console.warn(`CI lookup unavailable; running full validation: ${error.message}`);
}
appendFileSync(process.env.GITHUB_OUTPUT, `reuse=${Boolean(run)}\n`);
console.log(run ? `Reusing complete CI run ${run.id}, attempt ${run.run_attempt}, for ${sha}` : "Full release validation required");
