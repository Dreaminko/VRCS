import assert from "node:assert/strict";
import test from "node:test";

import { managedQwenReady } from "../src/settings/settings-derived.ts";
import type { AsrSettings, QwenModelRecord, QwenRuntimeStatus } from "../src/providers/types.ts";

const selected: AsrSettings["managed_qwen"] = {
  package_id: "qwen3-asr-0.6b-q8_0",
  device: "auto",
};
const installed: QwenModelRecord = {
  id: selected.package_id,
  engine: "qwen3-asr",
  repository: "Qwen/Qwen3-ASR-0.6B-GGUF",
  revision: "test",
  status: "installed",
  downloaded_bytes: 10,
  total_bytes: 10,
  progress: 1,
  error: null,
};
const runtime: QwenRuntimeStatus = {
  available: true,
  running: false,
  status: "not_loaded",
  error: null,
  device: null,
  fallback: null,
  gpu_devices: [],
  installation: {
    status: "installed",
    downloaded_bytes: 10,
    total_bytes: 10,
    progress: 1,
    error: null,
  },
};

test("local onboarding is ready with its installed package and an available idle runtime", () => {
  for (const device of ["auto", "cpu", "gpu"] as const) {
    assert.equal(managedQwenReady({ ...selected, device }, [installed], runtime, true), true);
  }
});

test("local onboarding waits for the selected package and runtime installation", () => {
  assert.equal(managedQwenReady(selected, [], runtime, true), false);
  assert.equal(managedQwenReady({ ...selected, package_id: "another-package" }, [installed], runtime, true), false);
  for (const status of ["not_downloaded", "downloading", "verifying", "corrupt", "error"] as const) {
    assert.equal(managedQwenReady(selected, [{ ...installed, status }], runtime, true), false);
  }
  assert.equal(managedQwenReady(selected, [installed], null, true), false);
  assert.equal(managedQwenReady(selected, [installed], { ...runtime, available: false }, true), false);
  assert.equal(managedQwenReady(selected, [installed], runtime, false), false);
});
