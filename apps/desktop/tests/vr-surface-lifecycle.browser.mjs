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
  .filter(Boolean).flatMap(root => [join(root, "Google/Chrome/Application/chrome.exe"), join(root, "Microsoft/Edge/Application/msedge.exe")]).find(existsSync);

test("surface sleep preserves reading position and visual receipts follow actual changes", {
  timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
}, async () => {
  const bundle = await build({
    stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
      import React from 'react';
      import { createRoot } from 'react-dom/client';
      import { flushSync } from 'react-dom';
      import { SurfaceHost } from './src/vr-surface/SurfaceHost';
      import { emit, calls } from 'fixture:tauri';
      import './src/styles.css';

      const passed = [];
      const check = (condition, message) => { if (!condition) throw new Error(message); passed.push(message); };
      const settle = () => new Promise(resolve => setTimeout(resolve, 80));
      const root = createRoot(document.getElementById('root'));
      const viewport = () => document.querySelector('[data-vr-page-viewport]');
      const paints = () => calls.filter(call => call.command === 'vr_surface_paint');
      const serial = () => paints().at(-1)?.args.paintSerial;
      let state = { key: { generation: 3, revision: 1 }, hovered: null, pressed: null, pointer: null,
        view: { kind: 'reader', dark: true, labels: {}, font_size: 28, opacity: 1,
          reader: { mode: 'vision', title: 'Vision', status: 'complete', detail: '', source: 'Question',
            translation: '**Answer** paragraph.\\n\\n'.repeat(100), translation_label: 'answer',
            block_index: 0, block_count: 1, page: 0, pinned: false, source_only: false, language_count: 2 } } };
      const publish = patch => { state = { ...state, ...patch }; flushSync(() => emit('vr-surface-view', state)); };
      const activity = (active, request, surface = 'reader') => flushSync(() => emit('vr-surface-activity', { active, request }, 'vr-surface-' + surface));
      async function run() {
        flushSync(() => root.render(<SurfaceHost surface="reader" />));
        await settle(); publish({}); await document.fonts.ready; await settle();
        check(serial() > 0, 'Initial pixels have a visual receipt');
        check(document.querySelectorAll('.vr-surface-marker i').length === 128, 'Marker fits two rows at the minimum surface width');
        const initial = serial(); publish({ hovered: 6 }); await settle();
        check(serial() > initial, 'Hover changes pixels without changing the content key');
        const hover = serial();
        emit('vr-surface-scroll', { key: state.key, delta: 0.5 }); await settle();
        check(viewport().scrollTop > 0 && serial() > hover, 'Scroll requests fresh captured pixels');
        const top = viewport().scrollTop;
        const idle = paints().length; await settle(); await settle();
        check(paints().length === idle, 'Static content does not continuously request captures');
        activity(false, 2); await settle();
        check(document.querySelector('main').dataset.active === 'false', 'Hidden surface pauses page work');
        check(calls.some(call => call.command === 'vr_surface_snapshot' && Math.abs(call.args.scrollTop - top) < 1), 'Reader sends a restorable position before cold release');
        const sleeping = paints().length;
        emit('vr-surface-scroll', { key: state.key, delta: 0.5 }); await settle();
        check(viewport().scrollTop === top && paints().length === sleeping, 'Hidden surfaces ignore input and stop paint requests');
        activity(true, 3); publish({ key: { generation: 3, revision: 2 } }); await settle();
        activity(false, 2); await settle();
        check(document.querySelector('main').dataset.active === 'true', 'Late sleep cannot override a newer wake');
        check(viewport().scrollTop === top, 'Warm wake preserves scroll position');
        const awakePaints = paints().length;
        flushSync(() => emit('vr-surface-view', { ...state, hovered: 7, activity: { active: false, request: 2 } }));
        await settle();
        check(paints().length === awakePaints && document.querySelector('main').dataset.active === 'true', 'Late state cannot change a newer activity request');
        activity(false, 100, 'menu'); await settle();
        const beforeForeignActivityScroll = serial();
        emit('vr-surface-scroll', { key: state.key, delta: 0.5 }); await settle();
        check(document.querySelector('main').dataset.active === 'true' && serial() > beforeForeignActivityScroll, 'Hiding another surface cannot pause the reader or block fresh pixels');
        root.unmount();
        const coldRoot = createRoot(document.getElementById('root'));
        flushSync(() => coldRoot.render(<SurfaceHost surface="reader" />)); await settle();
        publish({ key: { generation: 4, revision: 1 }, restore_scroll: top }); await settle();
        check(Math.abs(viewport().scrollTop - top) < 1, 'Cold wake restores the saved reading position');
        coldRoot.unmount();
        const animationRoot = createRoot(document.getElementById('root'));
        flushSync(() => animationRoot.render(<SurfaceHost surface="ocr_progress" />)); await settle();
        const progress = { scan_id: 1, status: 'working', detail: '', elapsed_seconds: 0,
          stages: ['active'], stage_labels: ['OCR'], translation_fraction: null };
        flushSync(() => emit('vr-surface-view', { key: { generation: 5, revision: 1 }, hovered: null, pressed: null, pointer: null,
          view: { kind: 'ocr_progress', dark: true, progress } }));
        await settle(); const running = paints().length; await settle();
        check(paints().length > running, 'Visible progress animation requests bounded refreshes');
        activity(false, 4, 'ocr_progress'); await settle(); const stopped = paints().length; await settle();
        check(paints().length === stopped, 'Hidden progress animation stops requesting captures');
        const animations = document.querySelector('main').getAnimations({ subtree: true });
        check(animations.every(animation => animation.playState === 'paused' || animation.playState === 'finished'), 'Hidden CSS animations are paused');
        animationRoot.unmount();
        document.getElementById('result').textContent = JSON.stringify({ passed });
      }
      run().catch(error => { document.getElementById('result').textContent = JSON.stringify({ error: error.stack, passed }); });
    ` },
    plugins: [{ name: "tauri-fixture", setup(build) {
      build.onResolve({ filter: /^(?:@tauri-apps\/api\/(?:core|event)|fixture:tauri)$/ }, () => ({ path: "tauri", namespace: "fixture" }));
      build.onLoad({ filter: /.*/, namespace: "fixture" }, () => ({ contents: `
        const listeners = new Map();
        export const calls = [];
        export async function invoke(command, args) { calls.push({ command, args }); return command === 'vr_surface_layout' || command === 'vr_surface_paint' ? true : null; }
        export async function listen(name, handler, options = {}) {
          const handlers = listeners.get(name) ?? new Set(); listeners.set(name, handlers);
          const subscription = { handler, target: options.target }; handlers.add(subscription);
          return () => handlers.delete(subscription);
        }
        export function emit(name, payload, label) {
          for (const { handler, target } of listeners.get(name) ?? []) {
            if (!target || target.kind === 'Any' || target.label === label) handler({ payload });
          }
        }
      ` }));
    } }],
    bundle: true, write: false, outfile: "surface-lifecycle.js", format: "esm", platform: "browser", jsx: "automatic", minify: true,
    loader: { ".png": "dataurl" },
  });
  const directory = mkdtempSync(join(testDirectory, ".vr-surface-lifecycle-"));
  try {
    const html = join(directory, "test.html");
    writeFileSync(html, '<!doctype html><style>' + bundle.outputFiles.find(file => file.path.endsWith(".css")).text
      + '</style><div id="root"></div><output id="result"></output><script type="module">'
      + bundle.outputFiles.find(file => file.path.endsWith(".js")).text.replace(/<\/script/gi, "<\\/script") + '</script>');
    const { stdout } = await promisify(execFile)(browser, [
      "--headless=new", "--dump-dom", "--virtual-time-budget=5000", "--disable-gpu", "--no-first-run",
      "--disable-background-networking", "--disable-extensions", "--window-size=1400,1200",
      "--user-data-dir=" + join(directory, "profile"), pathToFileURL(html).href,
    ], { timeout: 35000, maxBuffer: 8 * 1024 * 1024 });
    const output = stdout.match(/<output id="result">([^<]+)<\/output>/)?.[1];
    assert.ok(output, "The lifecycle fixture must finish");
    const result = JSON.parse(output);
    assert.equal(result.error, undefined, result.error);
    assert.equal(result.passed.length, 16, JSON.stringify(result));
  } finally {
    rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
});
