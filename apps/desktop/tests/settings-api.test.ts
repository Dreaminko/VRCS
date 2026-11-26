import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const bundle = await build({
  stdin: {
    contents: `export * from "./src/settings/api.ts";`,
    resolveDir: fileURLToPath(new URL("..", import.meta.url)),
  },
  bundle: true,
  write: false,
  format: "esm",
  platform: "node",
  define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
});

const { resetSettingsRevision, settingsApi } = await import(
  `data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString("base64")}`
) as typeof import("../src/settings/api.ts");

if (!("window" in globalThis)) {
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {
      setTimeout: globalThis.setTimeout.bind(globalThis),
      clearTimeout: globalThis.clearTimeout.bind(globalThis),
    },
  });
}

test("stale-revision PUT rejects with 409 instead of downgrading to GET", async (t) => {
  resetSettingsRevision();
  const calls: Array<{ method: string }> = [];
  const sample = { schema_version: 47 } as never;

  t.mock.method(globalThis, "fetch", async (_input: string | URL | Request, init?: RequestInit) => {
    const method = init?.method ?? "GET";
    calls.push({ method });
    if (calls.length === 1) {
      return new Response(JSON.stringify(sample), {
        status: 200,
        headers: { "Content-Type": "application/json", "X-VRCS-Config-Revision": "epoch-1:5" },
      });
    }
    return new Response(JSON.stringify(sample), {
      status: 200,
      headers: { "Content-Type": "application/json", "X-VRCS-Config-Revision": "epoch-1:4" },
    });
  });

  await settingsApi.settings();
  await assert.rejects(
    settingsApi.saveSettings(sample),
    (error: unknown) => Boolean(error && typeof error === "object" && (error as { status?: number }).status === 409),
  );
  assert.deepEqual(calls, [{ method: "GET" }, { method: "PUT" }]);
});

test("stale-revision GET retries once and updates revision only when current", async (t) => {
  resetSettingsRevision();
  const calls: Array<{ method: string }> = [];
  const sample = { schema_version: 47 } as never;

  t.mock.method(globalThis, "fetch", async (_input: string | URL | Request, init?: RequestInit) => {
    const method = init?.method ?? "GET";
    calls.push({ method });
    const rev = calls.length === 1 ? "epoch-2:5" : calls.length === 2 ? "epoch-2:4" : "epoch-2:6";
    return new Response(JSON.stringify(sample), {
      status: 200,
      headers: { "Content-Type": "application/json", "X-VRCS-Config-Revision": rev },
    });
  });

  await settingsApi.settings();
  await settingsApi.settings();
  assert.deepEqual(calls, [{ method: "GET" }, { method: "GET" }, { method: "GET" }]);
});
