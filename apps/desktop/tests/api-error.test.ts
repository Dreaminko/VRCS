import assert from "node:assert/strict";
import test from "node:test";
import { ApiError, apiErrorFromResponse } from "../src/api-error.ts";

test("preserves the structured Core error contract", async () => {
  const error = await apiErrorFromResponse(new Response(JSON.stringify({
    code: "asr.model.not_downloaded",
    params: { model: "small" },
    detail: "识别模型 small 尚未下载",
  }), {
    status: 409,
    statusText: "Conflict",
    headers: { "Content-Type": "application/json" },
  }));

  assert.ok(error instanceof ApiError);
  assert.equal(error.code, "asr.model.not_downloaded");
  assert.deepEqual(error.params, { model: "small" });
  assert.equal(error.detail, "识别模型 small 尚未下载");
  assert.equal(error.message, error.detail);
  assert.equal(error.status, 409);
});

test("uses a stable fallback for malformed error responses", async () => {
  const error = await apiErrorFromResponse(new Response("not json", {
    status: 502,
    statusText: "Bad Gateway",
  }));

  assert.equal(error.code, "http.unexpected_response");
  assert.deepEqual(error.params, { status: 502 });
  assert.equal(error.detail, "502 Bad Gateway");
});
