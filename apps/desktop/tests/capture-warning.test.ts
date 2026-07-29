import assert from "node:assert/strict";
import test from "node:test";
import { ApiError } from "../src/api-error.ts";
import { shouldShowVrchatNotRunningWarning } from "../src/capture-warning.ts";

test("shows a warning when VRChat-only capture cannot find VRChat", () => {
  assert.equal(
    shouldShowVrchatNotRunningWarning(
      new ApiError({
        code: "audio.vrchat_not_running",
        detail: "未发现正在运行的 VRChat，请先启动 VRChat",
        status: 503,
      }),
      true,
    ),
    true,
  );
});

test("keeps unrelated capture failures in the regular error banner", () => {
  assert.equal(
    shouldShowVrchatNotRunningWarning(
      new ApiError({
        code: "audio.device_unavailable",
        detail: "音频设备不可用",
        status: 503,
      }),
      true,
    ),
    false,
  );
  assert.equal(
    shouldShowVrchatNotRunningWarning(
      new ApiError({
        code: "audio.vrchat_not_running",
        detail: "未发现正在运行的 VRChat",
        status: 503,
      }),
      false,
    ),
    false,
  );
  assert.equal(
    shouldShowVrchatNotRunningWarning(
      new Error("未发现正在运行的 VRChat"),
      true,
    ),
    false,
  );
});
