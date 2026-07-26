import assert from "node:assert/strict";
import test from "node:test";
import { shouldShowVrchatNotRunningWarning } from "../src/capture-warning.ts";

test("shows a warning when VRChat-only capture cannot find VRChat", () => {
  assert.equal(
    shouldShowVrchatNotRunningWarning(
      "未发现正在运行的 VRChat，请先启动 VRChat",
      true,
    ),
    true,
  );
});

test("keeps unrelated capture failures in the regular error banner", () => {
  assert.equal(shouldShowVrchatNotRunningWarning("音频设备不可用", true), false);
  assert.equal(
    shouldShowVrchatNotRunningWarning("未发现正在运行的 VRChat", false),
    false,
  );
});
