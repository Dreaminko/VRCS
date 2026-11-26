import assert from "node:assert/strict";
import test from "node:test";
import { createSettingsAutosave } from "../src/settings/settings-autosave.ts";

test("updates immediately and keeps only the latest queued change", async () => {
  const events: string[] = [];
  let releaseFirst: (() => void) | undefined;
  const firstBlocked = new Promise<void>((resolve) => {
    releaseFirst = resolve;
  });
  const save = createSettingsAutosave<number>({
    persist: async (value) => {
      events.push(`persist:${value}`);
      if (value === 1) await firstBlocked;
      return value;
    },
    onOptimistic: (value) => events.push(`optimistic:${value}`),
    onCommit: (value) => events.push(`commit:${value}`),
    onError: () => events.push("error"),
  });

  const first = save(1);
  const second = save(2);
  const third = save(3);

  assert.deepEqual(events, ["optimistic:1", "persist:1", "optimistic:2", "optimistic:3"]);
  assert.equal(save.getDesired(), 3);

  releaseFirst?.();
  assert.deepEqual(await Promise.all([first, second, third]), [1, 3, 3]);
  assert.deepEqual(events, [
    "optimistic:1",
    "persist:1",
    "optimistic:2",
    "optimistic:3",
    "persist:3",
    "commit:3",
  ]);
});

test("only reports a failed save when it is still the latest change", async () => {
  const errors: string[] = [];
  const save = createSettingsAutosave<number>({
    persist: async (value) => {
      if (value === 1) throw new Error("old failure");
      return value;
    },
    onOptimistic: () => undefined,
    onCommit: () => undefined,
    onError: (reason) => errors.push(reason instanceof Error ? reason.message : String(reason)),
  });

  const first = save(1).catch(() => undefined);
  const second = save(2);
  await Promise.all([first, second]);

  assert.deepEqual(errors, []);
});

test("queued patches use confirmed settings and each has its own acknowledgement", async () => {
  let current = { port: 9001, enabled: false };
  let release!: () => void;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  const writes: typeof current[] = [];
  const save = createSettingsAutosave({
    getCurrent: () => current,
    persist: async (value) => {
      writes.push(value);
      if (writes.length === 1) await gate;
      current = value;
      return value;
    },
    onOptimistic: () => undefined,
    onCommit: () => undefined,
    onError: () => undefined,
  });
  const desktop = save({ ...current, port: 9002 });
  const enable = save.update((latest) => ({ ...latest, enabled: true }));
  const disable = save.update((latest) => ({ ...latest, enabled: false }));
  release();
  const [, enabled, disabled] = await Promise.all([desktop, enable, disable]);
  assert.deepEqual(enabled, { port: 9002, enabled: true });
  assert.deepEqual(disabled, { port: 9002, enabled: false });
  assert.equal(writes.length, 3);
});

test("coalesced patch updates combine pending edits and preserve concurrent non-coalesced patches and 409 retries", async () => {
  let current = { a: 0, b: 0, external: false };
  let release!: () => void;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  const writes: Array<typeof current> = [];
  let fail409 = false;
  const save = createSettingsAutosave({
    getCurrent: () => current,
    reload: async () => {
      current = { ...current, external: true };
      return current;
    },
    persist: async (value) => {
      writes.push(value);
      if (writes.length === 1) await gate;
      if (fail409) {
        fail409 = false;
        throw Object.assign(new Error("conflict"), { status: 409, code: "settings.stale" });
      }
      current = value;
      return value;
    },
    onOptimistic: () => undefined,
    onCommit: () => undefined,
    onError: () => undefined,
  });

  const first = save.update((s) => ({ ...s, a: 1 }), { coalesce: true });
  const second = save.update((s) => ({ ...s, b: 1 }), { coalesce: true });
  const third = save.update((s) => ({ ...s, a: 2 }), { coalesce: true });
  const vrPatch = save.update((s) => ({ ...s, external: true }));
  fail409 = true;
  release();
  await Promise.all([first, second, third, vrPatch]);
  assert.deepEqual(current, { a: 2, b: 1, external: true });
  assert.equal(writes.length, 4);
});

test("reports failed patch errors even when another patch is queued and skips retry on non-stale 409", async () => {
  let current = { a: 0, b: 0 };
  let release!: () => void;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  const errors: string[] = [];
  let reloads = 0;
  let calls = 0;
  const save = createSettingsAutosave({
    getCurrent: () => current,
    reload: async () => {
      reloads += 1;
      return current;
    },
    persist: async (value) => {
      calls += 1;
      if (calls === 1) {
        await gate;
        throw Object.assign(new Error("osc conflict"), { status: 409, code: "settings.osc_inbound_reload_failed" });
      }
      current = value;
      return value;
    },
    onOptimistic: () => undefined,
    onCommit: () => undefined,
    onError: (reason) => errors.push(reason instanceof Error ? reason.message : String(reason)),
  });

  const first = save.update((s) => ({ ...s, a: 1 })).catch(() => undefined);
  const second = save.update((s) => ({ ...s, b: 1 }));
  release();
  await Promise.all([first, second]);
  assert.deepEqual(errors, ["osc conflict"]);
  assert.equal(reloads, 0);
  assert.deepEqual(current, { a: 0, b: 1 });
});
