import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { build } from "esbuild";
import type { LearningItem } from "../src/learning/types.ts";
import type { learningApi } from "../src/learning/api.ts";

// Exercise hook state and deferred requests without a browser or extra dependencies.
async function inboxHarness(api: Partial<typeof learningApi>) {
  const slots: unknown[] = [];
  let index = 0;
  let changed = false;
  let effects: (() => void)[] = [];
  const hooks = {
    useState(initial: unknown) {
      const slot = index++;
      if (!(slot in slots)) slots[slot] = typeof initial === "function" ? initial() : initial;
      return [slots[slot], (value: unknown) => {
        const next = typeof value === "function" ? value(slots[slot]) : value;
        if (!Object.is(next, slots[slot])) changed = true;
        slots[slot] = next;
      }];
    },
    useRef(initial: unknown) {
      const slot = index++;
      if (!(slot in slots)) slots[slot] = { current: initial };
      return slots[slot];
    },
    useMemo<T>(callback: () => T, deps: unknown[]) {
      const slot = index++;
      const previous = slots[slot] as { deps: unknown[]; value: T } | undefined;
      if (!previous || deps.some((dep, i) => !Object.is(dep, previous.deps[i]))) {
        slots[slot] = { deps, value: callback() };
      }
      return (slots[slot] as { value: T }).value;
    },
    useCallback<T>(callback: T, deps: unknown[]) {
      return hooks.useMemo(() => callback, deps);
    },
    useEffect(callback: () => void, deps: unknown[]) {
      hooks.useMemo(() => { effects.push(callback); }, deps);
    },
  };
  const key = `learningInboxTest${Math.random().toString(36).slice(2)}`;
  Object.assign(globalThis, { [key]: { hooks, api } });
  const bundle = await build({
    entryPoints: [fileURLToPath(new URL("../src/learning/hooks/useLearningInbox.ts", import.meta.url))],
    bundle: true, write: false, format: "esm", platform: "node",
    plugins: [{ name: "hook-boundaries", setup(builder) {
      builder.onResolve({ filter: /^(react|react-i18next)$|\/api$|app-utils$/ }, (args) => ({ path: args.path, namespace: "test" }));
      builder.onLoad({ filter: /.*/, namespace: "test" }, (args) => ({ contents:
        args.path === "react" ? `export const {useState, useRef, useMemo, useCallback, useEffect} = globalThis.${key}.hooks;`
        : args.path === "react-i18next" ? 'const t = key => key; export const useTranslation = () => ({t});'
        : args.path.endsWith("api") ? `export const learningApi = globalThis.${key}.api;`
        : 'export const localizedError = error => String(error);',
      }));
    } }],
  });
  const { useLearningInbox } = await import(
    `data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString("base64")}`
  ) as typeof import("../src/learning/hooks/useLearningInbox.ts");
  Reflect.deleteProperty(globalThis, key);
  let current!: ReturnType<typeof useLearningInbox>;
  function flush() {
    for (let pass = 0; pass < 20; pass++) {
      changed = false;
      index = 0;
      current = useLearningInbox(false);
      const pending = effects;
      effects = [];
      pending.forEach((effect) => effect());
      if (!changed) return current;
    }
    throw new Error("Hook did not settle");
  }
  return { flush, get current() { return current; } };
}

function item(id: number, status: LearningItem["status"] = "collected"): LearningItem {
  return {
    id, kind: "sentence", status, source_text: "Text", working_text: "Text", selected_text: null,
    source_translation: null, source_language: "en", source_subtitle_ids: [], dictionary_entries: [],
    analysis: null, draft: null, anki_note_id: null, anki_sync_pending: false, created_at: "", updated_at: "",
  };
}

test("opening an old item does not move the server page cursor", async () => {
  const cursors: (number | undefined)[] = [];
  const inbox = await inboxHarness({
    async learningItems(options) {
      cursors.push(options?.beforeId);
      return cursors.length === 1
        ? Array.from({ length: 50 }, (_, index) => item(100 - index))
        : Array.from({ length: 49 }, (_, index) => item(50 - index));
    },
  });
  inbox.flush();
  await inbox.current.reload();
  inbox.flush();
  inbox.current.applyItem(item(1));
  inbox.flush();
  await inbox.current.loadMore();
  inbox.flush();
  assert.deepEqual(cursors, [undefined, 51]);
  assert.equal(inbox.current.items.length, 100);
  assert.equal(inbox.current.hasMore, false);
  await inbox.current.reload();
  assert.equal(cursors[2], undefined, "refresh starts from the first page");
});
