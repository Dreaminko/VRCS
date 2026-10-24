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

test("VR reader preserves scroll position and keeps the cursor out of captured surfaces", {
  timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
}, async () => {
  const bundle = await build({
    stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
      import React from 'react';
      import { createRoot } from 'react-dom/client';
      import { flushSync } from 'react-dom';
      import { SurfaceHost } from './src/vr-surface/SurfaceHost';
      import { VrPreview } from './src/vr-preview/VrPreview';
      import { emit, preview } from 'fixture:tauri';
      import './src/styles.css';

      const passed = [];
      const check = (condition, message) => {
        if (!condition) throw new Error(message);
        passed.push(message);
      };
      const settle = () => new Promise(resolve => setTimeout(resolve, 30));
      const root = createRoot(document.getElementById('root'));
      const viewport = () => document.querySelector('[data-vr-page-viewport]');
      const initial = {
        key: { generation: 1, revision: 1 }, hovered: null, pressed: null, pointer: null,
        view: { kind: 'reader', dark: true, labels: {}, font_size: 28, opacity: 1,
          reader: { mode: 'vision', title: 'Vision', status: 'complete', detail: '',
            source: 'Question line.\\n'.repeat(100), translation: '**Answer** paragraph.\\n\\n'.repeat(100),
            translation_label: 'answer', block_index: 0, block_count: 3, page: 0,
            pinned: false, source_only: false, language_count: 2 } }
      };
      let state = initial;
      const publish = patch => {
        state = { ...state, key: { ...state.key, revision: state.key.revision + 1 }, ...patch };
        flushSync(() => emit('vr-surface-view', state));
      };
      const reader = patch => publish({ view: { ...state.view, reader: { ...state.view.reader, ...patch } } });
      const scroll = (delta, key = state.key) => emit('vr-surface-scroll', { key, delta });

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
        scroll(0.5);
        check(Math.abs(viewport().scrollTop - viewport().clientHeight / 2) <= 1, 'Scroll delta uses viewport height');
        const top = viewport().scrollTop;
        for (const delta of [NaN, Infinity, -Infinity]) scroll(delta);
        scroll(0.5, { generation: 0, revision: 1 });
        scroll(0.5, { generation: 1, revision: 0 });
        check(viewport().scrollTop === top, 'Invalid and stale scroll events are ignored');
        publish({ hovered: 6 });
        reader({ pinned: true });
        reader({ translation: state.view.reader.translation + 'Additional answer.\\n\\n'.repeat(10) });
        await settle();
        check(viewport().scrollTop === top, 'Hover, pin and streamed text preserve position');
        scroll(1000);
        check(viewport().scrollTop === viewport().scrollHeight - viewport().clientHeight, 'Scroll clamps at the bottom');
        scroll(-1000);
        check(viewport().scrollTop === 0, 'Scroll clamps at the top');

        reader({ mode: 'ocr' });
        for (const patch of [{ block_index: 1 }, { translation_label: 'Japanese' }, { source_only: true }]) {
          scroll(0.5);
          reader(patch);
          check(viewport().scrollTop === 0, 'Content selection resets position: ' + Object.keys(patch)[0]);
        }
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
        root.unmount();
        document.getElementById('result').textContent = JSON.stringify({ passed });
      }
      run().catch(error => {
        root.unmount();
        document.getElementById('result').textContent = JSON.stringify({ error: error.stack, passed });
      });
    ` },
    plugins: [{ name: "tauri-fixture", setup(build) {
      build.onResolve({ filter: /^(?:@tauri-apps\/api\/(?:core|event)|fixture:tauri)$/ }, () => ({ path: "tauri", namespace: "fixture" }));
      build.onLoad({ filter: /.*/, namespace: "fixture" }, () => ({ contents: `
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
    assert.equal(result.passed.length, 19, JSON.stringify(result));
  } finally {
    rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
});
