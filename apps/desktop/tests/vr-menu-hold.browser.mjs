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

test("Menu hold feedback preserves hit bounds and the preview samples a stationary press", {
  timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
}, async () => {
  const bundle = await build({
    stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
      import React from 'react';
      import { createRoot } from 'react-dom/client';
      import { flushSync } from 'react-dom';
      import { VrSurface } from './src/vr-surface/VrSurface';
      import { SurfaceHost as Host } from './src/vr-surface/SurfaceHost';
      import { connectFixture } from 'fixture:surface';
      const SurfaceHost = ({ surface }) => <Host surface={surface} connect={connectFixture} />;
      import { VrPreview } from './src/vr-preview/VrPreview';
      import { preview, operations, emit } from 'fixture:tauri';
      import './src/styles.css';

      const root = createRoot(document.getElementById('root'));
      const passed = [];
      const check = (condition, message) => {
        if (!condition) throw new Error(message);
        passed.push(message);
      };
      const settle = (ms = 40) => new Promise(resolve => setTimeout(resolve, ms));
      const ring = () => document.querySelector('.vr-menu-hold-progress');
      const buttons = () => [...document.querySelectorAll('[data-vr-region]')];
      const bounds = () => JSON.stringify(buttons().map(button => {
        const { x, y, width, height } = button.getBoundingClientRect();
        return [button.dataset.vrRegion, x, y, width, height];
      }));

      async function run() {
        let layout;
        const onLayout = next => { layout = next; };
        for (const dark of [false, true]) {
          const state = structuredClone(preview.surface);
          state.view.controls.dark = dark;
          const render = (progress, pressed = 0) => {
            state.view.hold_progress = progress;
            state.pressed = pressed;
            flushSync(() => root.render(<VrSurface state={{ ...state }} onLayout={onLayout} />));
          };
          render(null, null);
          check(!ring(), 'No ring while idle: ' + dark);
          const surface = document.querySelector('.vr-surface').getBoundingClientRect();
          const menu = document.querySelector('.vr-menu-capsule').getBoundingClientRect();
          const menuBounds = [(menu.left - surface.left) / surface.width, (menu.top - surface.top) / surface.height,
            (menu.right - surface.left) / surface.width, (menu.bottom - surface.top) / surface.height];
          check(layout?.menu_bounds?.every((value, index) => Math.abs(value - menuBounds[index]) < 0.000001), 'Menu reports its visible capsule bounds: ' + dark);
          check(menu.bottom > buttons()[0].getBoundingClientRect().bottom, 'Menu bounds include background below the buttons: ' + dark);
          const original = bounds();
          for (const progress of [0, 0.5, 1]) {
            render(progress);
            const element = ring();
            check(element?.getAttribute('aria-valuenow') === String(progress * 100), 'Progress is exposed: ' + progress);
            check(element.lastElementChild.getAttribute('stroke-dashoffset') === String((1 - progress) * 100), 'Ring follows native progress: ' + progress);
            check(bounds() === original, 'Progress keeps all button bounds: ' + progress);
          }
          const rect = ring().getBoundingClientRect();
          const button = buttons()[0].getBoundingClientRect();
          check(rect.width === 160 && rect.height === 160 && rect.x === button.x && rect.y === button.y, 'Ring stays inside the button: ' + dark);
          const icon = buttons()[0].querySelector('svg').getBoundingClientRect();
          check(icon.width < rect.width && getComputedStyle(ring()).color !== 'rgba(0, 0, 0, 0)', 'Visible ring surrounds the icon: ' + dark);
          render(null, null);
          check(!ring(), 'Release clears feedback: ' + dark);
          render(0.5, 1);
          check(!ring(), 'Other buttons do not show the ring: ' + dark);
          for (const page of ['players', 'languages']) {
            state.view.ui = { page, group: 'current', player_page: 0, language_page: 0 };
            render(0.5, 0);
            check(ring()?.getAttribute('aria-valuenow') === '50' && ring().closest('[data-vr-region]')?.dataset.vrRegion === '0', 'Expanded settings preserve wrist relocation progress on the original control: ' + page + ' ' + dark);
            check(buttons().filter(button => button.closest('.vr-menu-capsule')).map(button => button.dataset.vrRegion).join(',') === '0,1,2,4,3', 'Expanded settings preserve quick-control region IDs: ' + page + ' ' + dark);
            const settingsIds = buttons().filter(button => button.closest('.vr-player-display-panel')).map(button => Number(button.dataset.vrRegion));
            for (const id of settingsIds) {
              render(0.5, id);
              check(!ring(), 'Settings controls do not show the wrist relocation ring: ' + page + ' ' + id + ' ' + dark);
            }
          }
        }

        flushSync(() => root.render(<SurfaceHost surface="menu" />));
        await settle();
        const publish = (hold_progress, pressed) => flushSync(() => emit('vr-surface-view', {
          ...preview.surface, pressed, view: { ...preview.surface.view, hold_progress },
        }));
        publish(0.5, 0);
        check(ring()?.getAttribute('aria-valuenow') === '50', 'Captured menu accepts progress under the same layout key');
        publish(1, 0);
        check(ring()?.getAttribute('aria-valuenow') === '100', 'Captured menu displays completed progress');
        publish(null, null);
        check(!ring(), 'Captured menu clears progress on release');

        preview.surface.pointer = [0.18, 0.5];
        flushSync(() => root.render(<VrPreview />));
        await settle();
        const target = document.querySelector('.vr-preview-react-surface');
        const previewScale = target.querySelector('.vr-surface').getBoundingClientRect().width / 1280;
        check(Math.abs(target.querySelector('.vr-surface-pointer').getBoundingClientRect().width / previewScale - 25) < 0.01, 'Menu preview uses the enlarged cursor');
        target.setPointerCapture = () => {};
        const button = target.querySelector('[data-vr-region="0"]').getBoundingClientRect();
        const pointer = (type, buttons) => flushSync(() => target.dispatchEvent(new PointerEvent(type, {
          bubbles: true, pointerType: 'mouse', pointerId: 1, buttons,
          clientX: button.x + button.width / 2, clientY: button.y + button.height / 2,
        })));
        pointer('pointermove', 0);
        pointer('pointerdown', 1);
        const presses = () => operations.filter(op => op.type === 'pointer' && op.input.buttons === 1).length;
        const before = presses();
        await settle(180);
        check(presses() > before, 'Stationary holds continue sending input');
        pointer('pointerup', 0);
        await settle();
        const after = presses();
        await settle(120);
        check(presses() === after, 'Release stops hold sampling');
        pointer('pointerdown', 1);
        window.dispatchEvent(new Event('blur'));
        await settle();
        const cancelled = presses();
        await settle(120);
        check(presses() === cancelled && operations.some(op => op.type === 'cancel'), 'Blur cancels hold sampling');
        root.unmount();
        document.getElementById('result').textContent = JSON.stringify({ passed });
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
        export const operations = [];
        export const preview = {
          session: 1, panel: 0, scene: 'normal', dark: false, mode: 'world', tracking: true,
          surface: { key: { generation: 1, revision: 1 }, hovered: null, pressed: null, pointer: null,
            view: { kind: 'menu', hold_progress: null, controls: { owner_epoch: 1, revision: 1, dark: false, labels: {},
              items: ['Wrist', 'Headset', 'OSC', 'Capture'].map(label => ({ label, requested: true, effective: true, available: true, busy: false, reason: null })) } } }
        };
        export async function invoke(command, args) {
          if (args?.operation) operations.push(args.operation);
          return command === 'vr_surface_ready' ? preview.surface : command === 'vr_surface_layout' ? true : preview;
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
    bundle: true, write: false, outfile: "menu-hold.js", format: "esm", platform: "browser", jsx: "automatic", minify: true,
    loader: { ".png": "dataurl" },
  });
  const directory = mkdtempSync(join(testDirectory, ".vr-menu-hold-"));
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
    assert.ok(output, "The menu hold fixture must finish");
    const result = JSON.parse(output);
    assert.equal(result.error, undefined, result.error);
    assert.ok(result.passed.includes("Blur cancels hold sampling"), JSON.stringify(result));
  } finally {
    assert.equal(dirname(directory), testDirectory);
    rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
});
