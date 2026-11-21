import { execFileSync } from "node:child_process";
import { appendFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

export function classifyChanges(files, full = false) {
  let rust = full;
  let frontend = full;
  for (const file of files) {
    const workflow = /^\.github\/workflows\/(ci|release|preview|nightly)\.yml$/.test(file);
    const releaseScript = /^scripts\/(?:build-.*release|install-linux-deps|verify-linux-package|release-metadata|write-release-metadata|test-release-metadata|find-release-ci|test-release-ci|ci-changes|test-ci-changes)\.(?:ps1|mjs|sh)$/.test(file);
    if (workflow || releaseScript || /^(core\/|crates\/|apps\/desktop\/src-tauri\/|packaging\/linux\/)/.test(file) || ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "scripts/run-tauri.ps1", "scripts/run-tauri.mjs"].includes(file) || /^scripts\/(?:check-rust|test-check-rust|prepare-vulkan-runtime)\.ps1$/.test(file)) rust = true;
    if (workflow || releaseScript || file.startsWith("apps/desktop/") || ["package.json", "package-lock.json", "scripts/check-i18n.mjs", "scripts/dev-vr.mjs", "scripts/dev-vr.test.mjs"].includes(file)) frontend = true;
  }
  return { rust, frontend };
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const base = process.env.BASE_SHA;
  const full = process.env.FULL_CHECK === "true" || !base || /^0+$/.test(base);
  const files = full ? [] : execFileSync("git", ["diff", "--name-only", `${base}...HEAD`], { encoding: "utf8" }).trim().split("\n");
  const result = classifyChanges(files, full);
  appendFileSync(process.env.GITHUB_OUTPUT, `rust=${result.rust}\nfrontend=${result.frontend}\n`);
}
