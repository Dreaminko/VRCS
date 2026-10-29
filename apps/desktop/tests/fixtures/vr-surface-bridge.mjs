// Native WebView message port for browser tests. Desktop/preview IPC stays separate.
import { connectSurfaceBridge } from "../../src/vr-surface/bridge.ts";
const listeners = new Set();
let surface = "reader";
const nonce = "browser-fixture-document";
export const surfaceCalls = [];
export const surfaceTesting = {};
export function bindSurface(name) { surface = name; }
export function connectFixture(name, signal) { bindSurface(name); return connectSurfaceBridge(name, signal); }
function receive(message) { for (const listener of [...listeners]) listener({ data: message }); }
export function emitSurface(name, payload, label) {
  if (label && label !== `vr-surface-${surface}`) return;
  if (name === "vr-surface-view") surface = payload.view.kind;
  receive({ version: 1, op: "event", nonce, event: name.replace("vr-surface-", ""), payload });
}
window.chrome ??= {};
window.chrome.webview = {
  addEventListener(_name, listener) { listeners.add(listener); },
  removeEventListener(_name, listener) { listeners.delete(listener); },
  postMessage(message) {
    if (message.op === "connect") {
      receive({ version: 1, op: "welcome", host: { surface, host_epoch: 1 }, nonce });
      return;
    }
    const { op, payload, request_id } = message;
    surfaceCalls.push({ command: `vr_surface_${op}`, args: { ...payload, paintSerial: payload.paint_serial, activityRequest: payload.activity_request, scrollTop: payload.scroll_top } });
    if (op === "layout") surfaceTesting.layout = payload.layout;
    receive({ version: 1, op: "reply", nonce, request_id, ok: true, value: op === "ready" ? null : true });
  },
};
