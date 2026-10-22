import assert from "node:assert/strict";
import { once } from "node:events";
import { createServer } from "node:http";
import test from "node:test";
import { startPreviewServer } from "./dev-vr.mjs";

test("VR preview uses a free port and leaves the existing server running", async (t) => {
  const occupied = createServer((_request, response) => response.end("existing server"));
  occupied.listen(0, "127.0.0.1");
  await once(occupied, "listening");
  t.after(() => new Promise((resolve) => occupied.close(resolve)));

  const port = occupied.address().port;
  const preview = await startPreviewServer(port);
  t.after(() => preview.close());
  const url = preview.resolvedUrls.local[0];

  assert.notEqual(Number(new URL(url).port), port);
  const page = await fetch(url);
  assert.equal(page.status, 200);
  assert.match(await page.text(), /src\/main\.tsx/);
  assert.equal(await (await fetch(`http://127.0.0.1:${port}`)).text(), "existing server");
});
