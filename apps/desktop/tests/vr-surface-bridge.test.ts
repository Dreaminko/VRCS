import assert from "node:assert/strict";
import test from "node:test";
import { connectSurfaceBridge, type WebviewMessagePort } from "../src/vr-surface/bridge.ts";

function fixture() {
  const sent: Record<string, unknown>[] = [];
  const listeners = new Set<(event: { data: unknown }) => void>();
  const lifecycle = new EventTarget();
  const port: WebviewMessagePort = {
    postMessage(message) { assert.ok(listeners.size, "subscribe before sending"); sent.push(message); },
    addEventListener(_name, listener) { listeners.add(listener); },
    removeEventListener(_name, listener) { listeners.delete(listener); },
  };
  Object.defineProperty(globalThis, "window", { configurable: true, value: { chrome: { webview: port }, addEventListener: lifecycle.addEventListener.bind(lifecycle), removeEventListener: lifecycle.removeEventListener.bind(lifecycle) } });
  const emit = (data: unknown) => { for (const listener of [...listeners]) listener({ data }); };
  const welcome = (nonce = "document-a", surface = "reader") => emit({ version: 1, op: "welcome", host: { surface, host_epoch: 2 }, nonce });
  const reply = (nonce: string, request: Record<string, unknown>, value: unknown) => emit({ version: 1, op: "reply", nonce, request_id: request.request_id, ok: true, value });
  return { sent, listeners, emit, welcome, reply, lifecycle };
}

test("surface bridge handshakes before requests and accepts only its document replies", async () => {
  const f = fixture();
  const abort = new AbortController();
  const connecting = connectSurfaceBridge("reader", abort.signal);
  assert.deepEqual([...f.sent], [{ version: 1, op: "connect" }]);
  f.welcome();
  const bridge = await connecting;
  const events: unknown[] = [];
  bridge.on("activity", activity => events.push(activity));
  f.emit({ version: 1, op: "event", nonce: "old-document", event: "activity", payload: { active: false, request: 4 } });
  f.emit({ version: 1, op: "event", nonce: "document-a", event: "activity", payload: { active: true, request: 5 } });
  assert.deepEqual(events, [{ active: true, request: 5 }]);
  const request = bridge.paint({ generation: 3, revision: 4 }, 8);
  const message = f.sent.at(-1)!;
  assert.deepEqual(message.payload, { key: { generation: 3, revision: 4 }, paint_serial: 8 });
  let settled = false;
  void request.then(() => { settled = true; });
  f.reply("old-document", message, true);
  await Promise.resolve();
  assert.equal(settled, false);
  f.reply("document-a", message, true);
  assert.equal(await request, true);
  bridge.dispose();
  assert.equal(f.listeners.size, 0);
});

test("surface bridge rejects a welcome bound to another surface", async () => {
  const f = fixture();
  const connecting = connectSurfaceBridge("reader", new AbortController().signal);
  f.welcome("document-a", "menu");
  await assert.rejects(connecting, /identity/);
  assert.equal(f.listeners.size, 0);
});

test("aborting a pending handshake removes listeners without waiting for welcome", async () => {
  const f = fixture();
  const abort = new AbortController();
  const connecting = connectSurfaceBridge("reader", abort.signal);
  abort.abort();
  await assert.rejects(connecting, /closed/);
  assert.equal(f.listeners.size, 0);
});

test("pagehide cancels the connection and its pending requests", async () => {
  const f = fixture();
  const connecting = connectSurfaceBridge("reader", new AbortController().signal);
  f.welcome();
  const bridge = await connecting;
  const pending = bridge.ready();
  const closed = assert.rejects(pending, /closed/);
  f.lifecycle.dispatchEvent(new Event("pagehide"));
  // Always dispose after the assertion, including a failing RED run.
  try { assert.equal(f.listeners.size, 0); }
  finally { bridge.dispose(); await closed; }
});

test("pending requests are bounded, time out, and release capacity", async context => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const f = fixture();
  const connecting = connectSurfaceBridge("reader", new AbortController().signal);
  f.welcome();
  const bridge = await connecting;
  const pending = Array.from({ length: 64 }, () => bridge.ready());
  const failures = Promise.all(pending.map(request => assert.rejects(request, /timed out/)));
  await assert.rejects(bridge.ready(), /Too many/);
  assert.equal(f.sent.length, 65);
  context.mock.timers.tick(4999);
  context.mock.timers.tick(1);
  await failures;
  const next = bridge.ready();
  f.reply("document-a", f.sent.at(-1)!, null);
  assert.equal(await next, null);
  bridge.dispose();
});

test("dispose cancels requests and reconnect never reuses request IDs", async () => {
  const f = fixture();
  const first = connectSurfaceBridge("reader", new AbortController().signal);
  f.welcome();
  const bridge = await first;
  const pending = bridge.ready();
  const old = f.sent.at(-1)!;
  bridge.dispose();
  await assert.rejects(pending, /closed/);
  const second = connectSurfaceBridge("reader", new AbortController().signal);
  f.welcome();
  const replacement = await second;
  const current = replacement.ready();
  const latest = f.sent.at(-1)!;
  assert.notEqual(latest.request_id, old.request_id);
  f.reply("document-a", old, { stale: true });
  f.reply("document-a", latest, null);
  assert.equal(await current, null);
  replacement.dispose();
  assert.equal(f.listeners.size, 0);
});

test("handshake timeout and native rejection clean up without leaking requests", async context => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const f = fixture();
  const missing = connectSurfaceBridge("reader", new AbortController().signal);
  const timeout = assert.rejects(missing, /timed out/);
  context.mock.timers.tick(20000);
  await timeout;
  assert.equal(f.listeners.size, 0);
  const connecting = connectSurfaceBridge("reader", new AbortController().signal);
  f.welcome();
  const bridge = await connecting;
  const request = bridge.ready();
  f.emit({ version: 1, op: "reply", nonce: "document-a", request_id: f.sent.at(-1)!.request_id, ok: false, error: "stale_session" });
  await assert.rejects(request, /stale_session/);
  bridge.dispose();
});
