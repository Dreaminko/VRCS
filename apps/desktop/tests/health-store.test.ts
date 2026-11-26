import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const bundle = await build({
  stdin: {
    contents: `export * from "./src/core-client/health-store.ts";`,
    resolveDir: fileURLToPath(new URL("..", import.meta.url)),
  },
  bundle: true,
  write: false,
  format: "esm",
  platform: "node",
  define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
});

const { getCurrentHealth, publishHealth } = await import(
  `data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString("base64")}`
) as typeof import("../src/core-client/health-store.ts");

test("publishHealth deduplicates identical health snapshots and reports changes", () => {
  const first = { status: "ok", capture_requested: false } as never;
  const duplicate = { status: "ok", capture_requested: false } as never;
  const changed = { status: "ok", capture_requested: true } as never;

  assert.equal(publishHealth(first), true);
  assert.equal(getCurrentHealth(), first);
  assert.equal(publishHealth(duplicate), false);
  assert.equal(getCurrentHealth(), first);
  assert.equal(publishHealth(changed), true);
  assert.equal(getCurrentHealth(), changed);
  assert.equal(publishHealth(null), true);
  assert.equal(publishHealth(null), false);
});
