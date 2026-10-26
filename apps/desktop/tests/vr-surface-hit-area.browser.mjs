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

test("VR hit contours exclude transparent borders and keep the dialogue toolbar separate", {
  timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
}, async () => {
  const bundle = await build({
    stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
      import React from 'react';
      import { createRoot } from 'react-dom/client';
      import { flushSync } from 'react-dom';
      import { VrSurface } from './src/vr-surface/VrSurface';
      import './src/styles.css';

      const root = createRoot(document.getElementById('root'));
      const passed = [];
      const check = (condition, message) => {
        if (!condition) throw new Error(message);
        passed.push(message);
      };
      const base = { key: { generation: 1, revision: 1 }, hovered: null, pressed: null, pointer: null };
      const controls = { owner_epoch: 1, revision: 1, dark: false, labels: {}, items: [] };
      const dialogue = { ...base, view: { kind: 'dialogue', controls, messages: [], mode: 'wrist', busy: false, error: null, font_size: 28, opacity: 1 } };
      const reader = { ...base, view: { kind: 'reader', dark: false, labels: {}, font_size: 28, opacity: 1,
        reader: { mode: 'ocr', source: 'Source', translation: 'Translation', title: 'Reader', status: '', detail: '', translation_label: 'answer',
          block_index: 0, block_count: 1, page: 0, pinned: false, source_only: false, language_count: 1 } } };
      const menu = { ...base, view: { kind: 'menu', controls, ui: { page: 'players', group: 'current', player_page: 0, language_page: 0 } } };
      let layout;
      const render = state => flushSync(() => root.render(<VrSurface state={state} onLayout={next => { layout = next; }} />));
      try {
        for (const dark of [false, true]) {
          dialogue.view.controls.dark = dark;
          render({ ...dialogue, view: { ...dialogue.view } });
          check(layout?.panels?.length === 2, 'Dialogue reports two separate panel contours: ' + dark);
          const toolbar = layout.panels[0];
          const body = layout.panels[1];
          check(toolbar.bounds[0] > 0 && toolbar.bounds[1] > 0 && body.bounds[2] < 1 && body.bounds[3] < 1,
            'Transparent outer margins are outside the contours: ' + dark);
          check(toolbar.bounds[3] < body.bounds[1], 'The transparent toolbar gap remains open: ' + dark);
          check(Math.abs(toolbar.radius[0] * 768 - 44) < 0.01 && Math.abs(body.radius[0] * 768 - 32) < 0.01,
            'Contours match the capsule and panel corner radii: ' + dark);
          const original = JSON.stringify(layout.panels);
          render({ ...dialogue, key: { generation: 1, revision: 2 }, view: { ...dialogue.view, messages: [{ side: 'left', text: 'Updated subtitles '.repeat(40) }] } });
          check(JSON.stringify(layout.panels) === original, 'Subtitle changes preserve the visible panel contour: ' + dark);
          render({ ...reader, view: { ...reader.view, dark } });
          check(layout.panels?.length === 1, 'Reader reports one outer panel contour: ' + dark);
          const panel = layout.panels[0];
          check(Math.abs(panel.bounds[0] * 1024 - 24) < 0.01 && Math.abs(panel.bounds[3] * 1024 - 1008) < 0.01,
            'Reader margins are excluded: ' + dark);
          check(Math.abs(panel.radius[0] * 1024 - 32) < 0.01, 'Reader rounded corners match its contour: ' + dark);
          menu.view.controls.dark = dark;
          render({ ...menu, view: { ...menu.view } });
          check(layout.panels?.length === 1, 'Expanded menu reports one settings contour beside its persistent capsule: ' + dark);
          check(layout.panels[0].bounds[3] < layout.menu_bounds[1], 'The transparent gap between settings and capsule remains open: ' + dark);
          check(Math.abs(layout.panels[0].radius[0] * 1280 - 32) < 0.01, 'Settings contour uses the existing panel radius: ' + dark);
          render({ ...menu, view: { ...menu.view, ui: { ...menu.view.ui, page: 'home' } } });
          check(!layout.panels && layout.menu_bounds[1] > 0.8, 'Collapsed menu does not capture the transparent settings area: ' + dark);
        }
        document.getElementById('result').textContent = JSON.stringify({ passed });
      } catch (error) {
        document.getElementById('result').textContent = JSON.stringify({ error: error.stack, passed });
      } finally { root.unmount(); }
    ` },
    bundle: true, write: false, outfile: "hit-area.js", format: "esm", platform: "browser", jsx: "automatic", minify: true,
    loader: { ".png": "dataurl" },
  });
  const directory = mkdtempSync(join(testDirectory, ".vr-hit-area-"));
  try {
    const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
    const stylesheet = bundle.outputFiles.find(file => file.path.endsWith(".css")).text;
    const html = join(directory, "test.html");
    writeFileSync(html, '<!doctype html><style>' + stylesheet + '</style><div id="root"></div><output id="result"></output><script type="module">'
      + javascript.replace(/<\/script/gi, "<\\/script") + '</script>');
    const { stdout } = await promisify(execFile)(browser, [
      "--headless=new", "--dump-dom", "--virtual-time-budget=2000", "--disable-gpu", "--no-first-run",
      "--disable-background-networking", "--disable-extensions", "--window-size=1400,1200",
      "--user-data-dir=" + join(directory, "profile"), pathToFileURL(html).href,
    ], { timeout: 35000, maxBuffer: 8 * 1024 * 1024 });
    const output = stdout.match(/<output id="result">([^<]+)<\/output>/)?.[1];
    assert.ok(output, "The hit contour fixture must finish");
    const result = JSON.parse(output);
    assert.equal(result.error, undefined, result.error);
    assert.equal(result.passed.length, 24);
  } finally {
    assert.equal(dirname(directory), testDirectory);
    rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
});
