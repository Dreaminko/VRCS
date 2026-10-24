import assert from "node:assert/strict";
import test from "node:test";

import { createCaptureController } from "../src/capture/capture-control.ts";
import { createSettingsAutosave } from "../src/settings/settings-autosave.ts";
import { applyControlAction, createControlExecutor } from "../src/vr-controls/actions.ts";
import { DEFAULT_VR_OVERLAY_SETTINGS } from "../src/settings/vr-overlay-settings.ts";
import type { Settings } from "../src/settings/types.ts";

function settings(): Settings {
  return {
    features: { vr_overlay: true, osc_chatbox: true },
    vr_overlay: structuredClone(DEFAULT_VR_OVERLAY_SETTINGS),
    osc: { enabled: false, port: 9001 },
  } as Settings;
}

test("content controls change only their requested layer", () => {
  const current = settings();
  const next = applyControlAction(current, { set_wrist_enabled: false });
  assert.equal(next.vr_overlay.wrist.enabled, false);
  assert.equal(next.vr_overlay.headset, current.vr_overlay.headset);
  assert.equal(next.osc, current.osc);
  assert.equal(applyControlAction(next, { set_wrist_enabled: false }), next);
});

test("OSC cannot enable a disabled feature", () => {
  const current = settings();
  current.features.osc_chatbox = false;
  assert.throws(() => applyControlAction(current, { set_osc_enabled: true }));
  assert.equal(current.osc.enabled, false);
});

test("dialogue placement saves anchor and follow preference together without changing other layers", async () => {
  const current = settings();
  let writes = 0;
  const action = { set_dialogue_placement: { mode: "world" as const, follow_view: true } };
  const execute = createControlExecutor({
    update: async (apply) => {
      writes++;
      const next = apply(current);
      assert.equal(next.vr_overlay.wrist.anchor_mode, "world");
      assert.equal(next.vr_overlay.wrist.follow_view, true);
      assert.equal(next.vr_overlay.headset, current.vr_overlay.headset);
      assert.equal(next.osc, current.osc);
      assert.equal(current.vr_overlay.wrist.anchor_mode, "wrist");
      assert.equal(current.vr_overlay.wrist.follow_view, false);
      assert.equal(applyControlAction(next, action), next);
      return next;
    },
    setCaptureRunning: async () => true,
  });
  await execute(action);
  assert.equal(writes, 1);
});

test("dialogue placement rejects a save that drops the follow preference", async () => {
  const current = settings();
  current.vr_overlay.wrist.anchor_mode = "world";
  const execute = createControlExecutor({ update: async () => current, setCaptureRunning: async () => true });
  await assert.rejects(execute({ set_dialogue_placement: { mode: "world", follow_view: true } }), /not applied/);
});

test("queued controls rebase a conflicted save and do not overwrite unrelated settings", async () => {
  let current = settings();
  let writes = 0;
  const save = createSettingsAutosave<Settings>({
    getCurrent: () => current,
    persist: async (next) => {
      writes++;
      if (writes === 1) throw { status: 409 };
      current = next;
      return next;
    },
    reload: async () => { current = { ...current, osc: { ...current.osc, port: 9002 } }; return current; },
    onOptimistic: () => undefined,
    onCommit: () => undefined,
    onError: () => undefined,
  });
  const execute = createControlExecutor({
    update: save.update,
    setCaptureRunning: async () => true,
  });
  await Promise.all([
    execute({ set_wrist_enabled: false }),
    execute({ set_headset_enabled: false }),
  ]);
  assert.equal(current.osc.port, 9002);
  assert.equal(current.vr_overlay.wrist.enabled, false);
  assert.equal(current.vr_overlay.headset.enabled, false);
  assert.equal(writes, 3);
});

test("a VR control preserves a desktop edit that is still saving", async () => {
  let current = settings();
  let release!: () => void;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  let writes = 0;
  const save = createSettingsAutosave<Settings>({
    getCurrent: () => current,
    persist: async (next) => {
      if (++writes === 1) await gate;
      current = next;
      return next;
    },
    onOptimistic: () => undefined,
    onCommit: () => undefined,
    onError: () => undefined,
  });
  const execute = createControlExecutor({
    update: save.update,
    setCaptureRunning: async () => true,
  });
  const desktop = save({ ...current, osc: { ...current.osc, port: 9002 } });
  const wrist = execute({ set_osc_enabled: true });
  await Promise.resolve();
  release();
  await Promise.all([desktop, wrist]);
  assert.equal(current.osc.port, 9002);
  assert.equal(current.osc.enabled, true);
});

test("a control cannot acknowledge a save that did not apply its target", async () => {
  const execute = createControlExecutor({
    update: async () => settings(),
    setCaptureRunning: async () => true,
  });
  await assert.rejects(execute({ set_osc_enabled: true }), /not applied/);
});

for (const source of ["desktop", "dashboard", "wrist_menu", "dev_preview"]) {
  test(`${source}: capture lock includes conversation creation and satisfied targets do nothing`, async () => {
    let running = false;
    let creates = 0;
    let starts = 0;
    const pending: boolean[] = [];
    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    const control = createCaptureController({
      available: () => true,
      running: () => running,
      newConversation: () => true,
      createConversation: async () => { creates++; await gate; return true; },
      setRunning: async (target) => { starts++; running = target; return true; },
      onPending: (value) => pending.push(value),
    });
    const first = control.setRunning(true);
    const second = control.setRunning(true);
    assert.equal(creates, 1);
    assert.deepEqual(pending, [true]);
    release();
    assert.deepEqual(await Promise.all([first, second]), [true, true]);
    assert.equal(starts, 1);
    assert.equal(await control.setRunning(true), true);
    assert.equal(creates, 1);
    assert.equal(starts, 1);
    await control.setRunning(false);
    assert.equal(creates, 1);
    assert.equal(running, false);
    assert.deepEqual(pending, [true, false, true, false]);
  });
}

test("failed conversation creation never starts capture and releases pending", async () => {
  let starts = 0;
  const control = createCaptureController({
    available: () => true, running: () => false, newConversation: () => true,
    createConversation: async () => false,
    setRunning: async () => { starts++; return true; },
    onPending: () => undefined,
  });
  assert.equal(await control.setRunning(true), false);
  assert.equal(starts, 0);
  assert.equal(await control.setRunning(false), true);
});
