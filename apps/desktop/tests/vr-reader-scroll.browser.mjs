import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { promisify } from "node:util";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const testDirectory = dirname(fileURLToPath(import.meta.url));
const browser = [process.env.PROGRAMFILES, process.env["PROGRAMFILES(X86)"], process.env.LOCALAPPDATA]
  .filter(Boolean).flatMap(root => [
    join(root, "Google/Chrome/Application/chrome.exe"),
    join(root, "Microsoft/Edge/Application/msedge.exe"),
  ]).find(existsSync);

test("VR reader and dialogue scrolling preserves position and bounds caption rendering", {
  timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
}, async () => {
  const bundle = await build({
    stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
      import React from 'react';
      import { createRoot } from 'react-dom/client';
      import { flushSync } from 'react-dom';
      import { SurfaceHost as Host } from './src/vr-surface/SurfaceHost';
      import { connectFixture } from 'fixture:surface';
      const SurfaceHost = ({ surface }) => <Host surface={surface} connect={connectFixture} />;
      import { VrPreview } from './src/vr-preview/VrPreview';
      import { emit, preview } from 'fixture:tauri';
      import './src/styles/base.css';

      const passed = [];
      const check = (condition, message) => {
        if (!condition) throw new Error(message);
        passed.push(message);
      };
      const settle = async () => {
        await new Promise(resolve => setTimeout(resolve, 30));
        frame(); await Promise.resolve(); frame(); await Promise.resolve();
      };
      let frameId = 0;
      const frames = new Map();
      window.requestAnimationFrame = callback => { frames.set(++frameId, callback); return frameId; };
      window.cancelAnimationFrame = id => frames.delete(id);
      const frame = () => {
        const pending = [...frames.values()]; frames.clear();
        flushSync(() => pending.forEach(callback => callback(performance.now())));
      };
      const root = createRoot(document.getElementById('root'));
      const viewport = () => document.querySelector('[data-vr-page-viewport]');
      const initial = {
        key: { generation: 1, revision: 1 }, scroll_key: { generation: 1, revision: 1 }, hovered: null, pressed: null, pointer: null,
        view: { kind: 'reader', dark: true, labels: {}, font_size: 28, opacity: 1,
          reader: { mode: 'vision', title: 'Vision', status: 'complete', detail: '',
            source: 'Question line.\\n'.repeat(100), translation: '**Answer** paragraph.\\n\\n'.repeat(100),
            translation_label: 'answer', block_index: 0, block_count: 3, page: 0,
            pinned: false, source_only: false, language_count: 2 } }
      };
      const caption = index => ({ speaker: 'Player ' + index, text: 'Caption ' + index + '\\nTranslation '.repeat(8), side: index % 2 ? 'right' : 'left' });
      const dialogue = {
        ...initial, view: { kind: 'dialogue', controls: { owner_epoch: 1, revision: 1, dark: true, labels: {}, items: [] },
          messages: Array.from({ length: 10 }, (_, i) => caption(i)), max_entries: 10, max_visible_players: 0,
          mode: 'wrist', follow_view: false, busy: false, error: null, font_size: 28, opacity: 0.94 }
      };
      let state = initial;
      const publish = patch => {
        state = { ...state, key: { ...state.key, revision: state.key.revision + 1 }, ...patch };
        flushSync(() => emit('vr-surface-view', state));
        frame();
      };
      const reader = patch => {
        const reset = ['mode', 'block_index', 'source', 'translation_label', 'source_only', 'page'].some(key => key in patch && patch[key] !== state.view.reader[key]);
        publish({ view: { ...state.view, reader: { ...state.view.reader, ...patch } },
          ...(reset ? { scroll_key: { ...state.key, revision: state.key.revision + 1 } } : {}) });
      };
      const scroll = (delta, key = state.scroll_key) => emit('vr-surface-scroll', { key, delta });

      async function run() {
        flushSync(() => root.render(<SurfaceHost surface="reader" />));
        await settle();
        flushSync(() => emit('vr-surface-view', state));
        await document.fonts.ready;
        await settle();
        publish({ pointer: [0.5, 0.5] });
        flushSync(() => emit('vr-surface-pointer', { key: state.key, point: [0.2, 0.3] }));
        check(!document.querySelector('.vr-surface-pointer'), 'In-game reader leaves the cursor to OpenVR');
        check(viewport().scrollHeight > viewport().clientHeight + 1000, 'Long reader content overflows');
        check(!document.querySelector('[data-vr-region="2"], [data-vr-region="3"]'), 'Page buttons are removed');
        await settle();
        scroll(0.2); scroll(0.3);
        check(viewport().scrollTop === 0 && frames.size === 1, 'Input bursts wait for one animation frame');
        frame();
        check(Math.abs(viewport().scrollTop - viewport().clientHeight / 2) <= 1, 'Scroll delta uses viewport height');
        const top = viewport().scrollTop;
        for (const delta of [NaN, Infinity, -Infinity]) scroll(delta);
        scroll(0.5, { generation: 0, revision: 1 });
        scroll(0.5, { generation: 1, revision: 0 });
        frame();
        check(viewport().scrollTop === top, 'Invalid and stale scroll events are ignored');
        publish({ hovered: 6 });
        reader({ pinned: true });
        reader({ translation: state.view.reader.translation + 'Additional answer.\\n\\n'.repeat(10) });
        await settle();
        check(viewport().scrollTop === top, 'Hover, pin and streamed text preserve position');
        scroll(0.1);
        reader({ translation: state.view.reader.translation + 'Streamed while scrolling.' });
        frame();
        check(Math.abs(viewport().scrollTop - top - viewport().clientHeight * 0.1) <= 1, 'Pending scroll survives streamed text');
        scroll(1000);
        frame();
        check(viewport().scrollTop === viewport().scrollHeight - viewport().clientHeight, 'Scroll clamps at the bottom');
        const bottomBeforeReverse = viewport().scrollTop;
        scroll(10); scroll(-0.1); frame();
        check(Math.abs(viewport().scrollTop - bottomBeforeReverse + viewport().clientHeight * 0.1) <= 1, 'Reversal at the boundary retains valid movement');
        scroll(-1000);
        frame();
        check(viewport().scrollTop === 0, 'Scroll clamps at the top');

        reader({ mode: 'ocr' });
        for (const patch of [{ block_index: 1 }, { translation_label: 'Japanese' }, { source_only: true }]) {
          scroll(0.5);
          reader(patch);
          frame();
          check(viewport().scrollTop === 0, 'Content selection resets position: ' + Object.keys(patch)[0]);
        }
        const selectedKey = state.scroll_key;
        reader({ source: 'A different question.\\n'.repeat(100) });
        scroll(0.5, selectedKey); frame();
        check(viewport().scrollTop === 0, 'Old viewport input cannot scroll a new selection');
        scroll(0.5);
        flushSync(() => emit('vr-surface-activity', { active: false, request: 1 }));
        frame();
        check(viewport().scrollTop === 0 && frames.size === 0, 'Sleep cancels pending scroll without a trailing frame');
        flushSync(() => emit('vr-surface-activity', { active: true, request: 2 }));
        reader({ page: 1 });
        check(viewport().scrollTop > 0, 'Custom SteamVR page actions remain supported');
        check(getComputedStyle(viewport(), '::-webkit-scrollbar').width === '8px', 'Desktop scrollbar width is reused');
        check(getComputedStyle(viewport(), '::-webkit-scrollbar-thumb').borderRadius === '999px', 'Desktop scrollbar rounding is reused');

        preview.surface = { ...initial, pointer: [0.5, 0.5] };
        flushSync(() => root.render(<VrPreview />));
        await settle();
        const panel = document.querySelector('.vr-preview-react-surface');
        check(panel.querySelector('.vr-surface-pointer'), 'Desktop preview keeps its local cursor');
        panel.dispatchEvent(new WheelEvent('wheel', { bubbles: true, deltaY: 100 }));
        check(viewport().scrollTop === 100, 'Desktop preview wheel scrolls reader content');
        panel.dispatchEvent(new WheelEvent('wheel', { bubbles: true, deltaY: 1, deltaMode: 2 }));
        check(Math.abs(viewport().scrollTop - 100 - viewport().clientHeight) <= 1, 'Preview supports page-mode wheel input');
        const previewTop = viewport().scrollTop;
        const bounds = panel.getBoundingClientRect();
        panel.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerType: 'mouse', buttons: 2,
          clientX: bounds.left + 100, clientY: bounds.top + 100 }));
        panel.dispatchEvent(new WheelEvent('wheel', { bubbles: true, deltaY: 100 }));
        await settle();
        check(viewport().scrollTop === previewTop, 'Right-button wheel leaves content position unchanged');
        check(document.querySelector('input[type="range"]').value === '0.9', 'Right-button wheel still adjusts panel depth');

        flushSync(() => root.render(<SurfaceHost surface="dialogue" />));
        await settle();
        state = dialogue;
        flushSync(() => emit('vr-surface-view', state));
        await settle();
        check(viewport(), 'Dialogue exposes a scrollable viewport');
        const atBottom = () => Math.abs(viewport().scrollHeight - viewport().clientHeight - viewport().scrollTop) <= 1;
        check(viewport().scrollTop > 1000 && atBottom(), 'Dialogue starts at the latest bubbles');
        const nodes = [...document.querySelectorAll('.vr-dialogue-bubble')];
        const beforeStreamingScroll = viewport().scrollTop;
        scroll(-0.5);
        publish({ hovered: 1 });
        publish({ view: { ...state.view, messages: [...state.view.messages.slice(0, -1), { ...state.view.messages.at(-1), text: state.view.messages.at(-1).text + '\\nMore translation'.repeat(10) }] } });
        frame();
        const dialogueTop = viewport().scrollTop;
        check(Math.abs(dialogueTop - beforeStreamingScroll + viewport().clientHeight * 0.5) <= 1 && !atBottom(), 'Queued controller scroll survives incoming captions before the frame');
        await settle();
        check(viewport().scrollTop === dialogueTop, 'Hover and immediate streamed captions do not jump while reading history');
        for (const delta of [NaN, Infinity, -Infinity]) scroll(delta);
        scroll(-1, { generation: 0, revision: 1 });
        frame();
        check(viewport().scrollTop === dialogueTop, 'Dialogue ignores invalid and stale scroll events');
        const mutations = [];
        const observer = new MutationObserver(records => mutations.push(...records));
        observer.observe(document.querySelector('.vr-surface'), { childList: true, subtree: true, attributes: true, characterData: true });
        scroll(1000);
        frame();
        await settle();
        observer.disconnect();
        check(!mutations.length && nodes.every((node, index) => node === document.querySelectorAll('.vr-dialogue-bubble')[index]), 'Scrolling does not rerender caption DOM');
        publish({ view: { ...state.view, messages: [...state.view.messages, caption(10)] } });
        await settle();
        check(atBottom(), 'Returning to the bottom resumes following new captions');
        for (let i = 11; i < 300; i++) {
          publish({ view: { ...state.view, messages: [...state.view.messages.slice(-127), caption(i)] } });
          check(document.querySelectorAll('.vr-dialogue-bubble').length === 10, 'Caption DOM remains capped at ten: ' + i);
        }
        await settle();
        check(atBottom() && document.querySelector('.vr-dialogue-messages').textContent.includes('Caption 299'), 'Long-running updates retain the latest bounded captions');
        scroll(-1000);
        frame();
        await settle();
        check(viewport().scrollTop === 0 && document.querySelector('.vr-dialogue-bubble').getBoundingClientRect().top >= viewport().getBoundingClientRect().top - 1, 'Oldest retained bubble is reachable');
        publish({ view: { ...state.view, max_entries: 3 } });
        check(document.querySelectorAll('.vr-dialogue-bubble').length === 3, 'A lower configured history limit applies immediately');
        preview.surface = dialogue;
        flushSync(() => root.render(<VrPreview />));
        await settle();
        const dialoguePanel = document.querySelector('.vr-preview-react-surface');
        const bottom = viewport().scrollTop;
        dialoguePanel.dispatchEvent(new WheelEvent('wheel', { bubbles: true, deltaY: -100 }));
        check(viewport().scrollTop === bottom - 100, 'Desktop preview wheel scrolls the same dialogue bubbles');
        const dialogueBounds = dialoguePanel.getBoundingClientRect();
        dialoguePanel.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerType: 'mouse', buttons: 2,
          clientX: dialogueBounds.left + 100, clientY: dialogueBounds.top + 100 }));
        dialoguePanel.dispatchEvent(new WheelEvent('wheel', { bubbles: true, deltaY: 100 }));
        await settle();
        check(viewport().scrollTop === bottom - 100 && document.querySelector('input[type="range"]').value === '0.9', 'Dialogue right-button wheel adjusts depth without scrolling');
        root.unmount();
        document.getElementById('result').textContent = JSON.stringify({ passed, complete: true });
      }
      run().catch(error => {
        root.unmount();
        document.getElementById('result').textContent = JSON.stringify({ error: error.stack, passed });
      });
    ` },
    plugins: [{ name: "tauri-fixture", setup(build) {
      build.onResolve({ filter: /^fixture:surface$/ }, () => ({ path: join(testDirectory, "fixtures/vr-surface-bridge.mjs") }));
      build.onResolve({ filter: /^(?:@tauri-apps\/api\/(?:core|event)|fixture:tauri)$/ }, () => ({ path: "tauri", namespace: "fixture" }));
      build.onLoad({ filter: /.*/, namespace: "fixture" }, () => ({ contents: `
        import { emitSurface, surfaceCalls, surfaceTesting } from 'fixture:surface';
        const listeners = new Map();
        export const preview = { session: 1, panel: 4, scene: 'long', dark: true, mode: 'wrist' };
        export async function invoke(command) {
          return command.startsWith('vr_preview_') ? preview : command === 'vr_surface_layout' ? true : null;
        }
        export async function listen(name, handler) {
          const handlers = listeners.get(name) ?? new Set();
          listeners.set(name, handlers); handlers.add(handler);
          return () => handlers.delete(handler);
        }
        export function emit(name, payload) {
          if (name.startsWith("vr-surface-")) { emitSurface(name, payload); return; }
          for (const handler of listeners.get(name) ?? []) handler({ payload });
        }
      ` }));
    } }],
    bundle: true, write: false, outfile: "reader-scroll.js", format: "esm", platform: "browser", jsx: "automatic", minify: true,
    loader: { ".png": "dataurl" },
  });
  const directory = mkdtempSync(join(testDirectory, ".vr-reader-scroll-"));
  try {
    const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
    const stylesheet = bundle.outputFiles.find(file => file.path.endsWith(".css")).text;
    const html = join(directory, "test.html");
    writeFileSync(html, '<!doctype html><style>' + stylesheet + '</style><div id="root"></div><output id="result"></output><script type="module">'
      + javascript.replace(/<\/script/gi, "<\\/script") + '</script>');
    const { stdout } = await promisify(execFile)(browser, [
      "--headless=new", "--dump-dom", "--virtual-time-budget=3000", "--disable-gpu", "--no-first-run",
      "--disable-background-networking", "--disable-extensions", "--window-size=1400,1200",
      "--user-data-dir=" + join(directory, "profile"), pathToFileURL(html).href,
    ], { timeout: 35000, maxBuffer: 8 * 1024 * 1024 });
    const output = stdout.match(/<output id="result">([^<]+)<\/output>/)?.[1];
    assert.ok(output, "The reader fixture must finish");
    const result = JSON.parse(output);
    assert.equal(result.error, undefined, result.error);
    assert.equal(result.complete, true, JSON.stringify(result));
  } finally {
    rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
});
