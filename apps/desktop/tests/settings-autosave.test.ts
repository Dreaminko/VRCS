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
