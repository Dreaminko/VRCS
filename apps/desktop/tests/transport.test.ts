import assert from "node:assert/strict";
import { createServer, type ServerResponse } from "node:http";
import { fileURLToPath } from "node:url";
import test, { type TestContext } from "node:test";
import { build } from "esbuild";

// Bundle the browser entry so Node can load extensionless imports and Vite's env.
const bundle = await build({
  stdin: {
    contents: `
      export * from "./src/core-client/transport.ts";
      export * from "./src/learning/api.ts";
    `,
    resolveDir: fileURLToPath(new URL("..", import.meta.url)),
  },
  bundle: true,
  write: false,
  format: "esm",
  platform: "node",
  define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
});
const { AI_REQUEST_TIMEOUT_MS, learningApi, request, timedCoreFetch } = await import(
  `data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString("base64")}`
) as typeof import("../src/core-client/transport.ts") & typeof import("../src/learning/api.ts");

async function stalledResponse(t: TestContext, status = 200) {
  let body!: ServerResponse;
  const server = createServer((_req, response) => {
    body = response;
    response.writeHead(status, { "Content-Type": "application/json" });
    response.flushHeaders();
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(async () => {
    server.closeAllConnections();
    await new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  });
  const address = server.address();
  assert.ok(address && typeof address !== "string");

  const timers = new Map<number, { callback: () => void; delay: number }>();
  let nextTimer = 0;
  const windowDescriptor = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {
      setTimeout(callback: () => void, delay: number) {
        const id = ++nextTimer;
        timers.set(id, { callback, delay });
        return id;
      },
      clearTimeout(id: number) { timers.delete(id); },
    },
  });
  t.after(() => {
    if (windowDescriptor) Object.defineProperty(globalThis, "window", windowDescriptor);
    else Reflect.deleteProperty(globalThis, "window");
  });

  const caller = new AbortController();
  const addListener = t.mock.method(caller.signal, "addEventListener");
  const removeListener = t.mock.method(caller.signal, "removeEventListener");
  let fetchSignal!: AbortSignal;
  let receiveHeaders!: (response: Response) => void;
  const headers = new Promise<Response>((resolve) => { receiveHeaders = resolve; });
  const nativeFetch = globalThis.fetch;
  t.mock.method(globalThis, "fetch", async (input: string | URL | Request, init?: RequestInit) => {
    assert.ok(init?.signal);
    fetchSignal = init.signal;
    const response = await nativeFetch(`http://127.0.0.1:${address.port}${new URL(String(input)).pathname}`, init);
    receiveHeaders(response);
    return response;
  });

  return {
    caller,
    headers,
    timers,
    get signal() { return fetchSignal; },
    finish(value: string) { body.end(value); },
    expire() {
      assert.equal(timers.size, 1);
      [...timers.values()][0].callback();
    },
    assertCleanup() {
      assert.equal(timers.size, 0, "request timer must be cleared");
      assert.equal(addListener.mock.callCount(), 1);
      assert.equal(removeListener.mock.callCount(), 1);
      assert.equal(removeListener.mock.calls[0].arguments[0], "abort");
      assert.equal(removeListener.mock.calls[0].arguments[1], addListener.mock.calls[0].arguments[1]);
    },
  };
}

for (const status of [200, 503]) {
  test(`JSON request times out while the ${status} body is delayed after headers`, { timeout: 5000 }, async (t) => {
    const response = await stalledResponse(t, status);
    const result = request("/delayed", { signal: response.caller.signal, timeoutMs: 37 });
    const rejected = assert.rejects(result, { name: "TimeoutError", message: "Request timed out" });
    await response.headers;
    assert.equal(response.timers.size, 1, "headers must not clean up the JSON request timer");
    assert.equal([...response.timers.values()][0].delay, 37);
    response.expire();
    await rejected;
    assert.equal(response.signal.aborted, true);
    response.assertCleanup();
  });

  test(`caller abort cancels the ${status} JSON body after headers`, { timeout: 5000 }, async (t) => {
    const response = await stalledResponse(t, status);
    const result = request("/delayed", { signal: response.caller.signal });
    const reason = new DOMException("Caller cancelled", "AbortError");
    const rejected = assert.rejects(result, (error) => error === reason);
    await response.headers;
    response.caller.abort(reason);
    await rejected;
    assert.equal(response.signal.reason, reason);
    response.assertCleanup();
  });
}

test("JSON success cleans up only after the complete body and detaches caller abort", { timeout: 5000 }, async (t) => {
  const response = await stalledResponse(t);
  const result = request<{ answer: string }>("/success", { signal: response.caller.signal });
  await response.headers;
  assert.equal(response.timers.size, 1);
  response.finish('{"answer":"ready"}');
  assert.deepEqual(await result, { answer: "ready" });
  response.assertCleanup();
  response.caller.abort();
  assert.equal(response.signal.aborted, false);
});

test("complete error bodies keep the structured API error and clean up", { timeout: 5000 }, async (t) => {
  const response = await stalledResponse(t, 503);
  const result = request("/error", { signal: response.caller.signal });
  const rejected = assert.rejects(result, {
    name: "ApiError", code: "llm.request_failed", detail: "Provider rejected the request",
    status: 503, params: { provider: "test" },
  });
  await response.headers;
  assert.equal(response.timers.size, 1);
  response.finish(JSON.stringify({
    code: "llm.request_failed", detail: "Provider rejected the request", params: { provider: "test" },
  }));
  await rejected;
  response.assertCleanup();
});

for (const status of [200, 502]) {
  test(`malformed ${status} JSON cleans up without masking the parse/error fallback`, { timeout: 5000 }, async (t) => {
    const response = await stalledResponse(t, status);
    const result = request("/malformed", { signal: response.caller.signal });
    const rejected = assert.rejects(result, status === 200
      ? { name: "SyntaxError" }
      : { name: "ApiError", code: "http.unexpected_response", status: 502 });
    await response.headers;
    response.finish("not json");
    await rejected;
    response.assertCleanup();
  });
}

test("timedCoreFetch still releases timeout and caller abort at headers for body consumers", { timeout: 5000 }, async (t) => {
  const response = await stalledResponse(t);
  const raw = await timedCoreFetch("/download", { signal: response.caller.signal });
  response.assertCleanup();
  response.caller.abort();
  assert.equal(response.signal.aborted, false);
  response.finish("download body");
  assert.equal(await raw.text(), "download body");
});

test("analyzeLearningItem applies the extended AI timeout and forwards caller abort", { timeout: 5000 }, async (t) => {
  const response = await stalledResponse(t);
  const result = learningApi.analyzeLearningItem(12, {
    task_type: "sentence_analysis",
    profile_id: "profile-one",
    model: "model-one",
    explanation_language: "en-US",
    level: "intermediate",
  }, response.caller.signal);
  await response.headers;
  assert.equal([...response.timers.values()][0]?.delay, AI_REQUEST_TIMEOUT_MS);
  response.finish('{"id":12}');
  assert.deepEqual(await result, { id: 12 });
  response.assertCleanup();
});

test("a fetch failure cleans up and preserves the original error", async (t) => {
  const response = await stalledResponse(t);
  const reason = new TypeError("Network unavailable");
  t.mock.method(globalThis, "fetch", async () => { throw reason; });
  await assert.rejects(request("/offline", { signal: response.caller.signal }), (error) => error === reason);
  response.assertCleanup();
});
