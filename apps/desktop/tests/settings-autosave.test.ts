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
