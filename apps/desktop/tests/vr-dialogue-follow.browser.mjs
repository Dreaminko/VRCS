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

test("dialogue follow button sits left of pin and exposes only valid hit regions", {
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
      let layout;
      let revision = 0;
      const render = (dark, mode, follow_view, busy = false, hovered = null) => flushSync(() => root.render(<VrSurface
        state={{ key: { generation: 1, revision: ++revision }, hovered, pressed: null, pointer: null,
          view: { kind: 'dialogue', controls: { owner_epoch: 1, revision: 1, dark,
            labels: { followView: '跟随视角', anchorWrist: '固定在手上', anchorWorld: '空间固定', dialogueTitle: '手腕字幕' }, items: [] },
            messages: [{ side: 'left', text: '你好，欢迎使用 VRCS。' }, { side: 'right', text: 'The dialogue panel follows your view.' }],
            mode, follow_view, busy, error: null, font_size: 28, opacity: 0.94 } }}
        onLayout={next => { layout = next; }} />));
      const follow = () => document.querySelector('[data-vr-region="3"]');
      try {
        for (const dark of [false, true]) {
          render(dark, 'wrist', false);
          check(follow().disabled, 'Wrist mode disables following: ' + dark);
          check(!layout.regions.some(region => region.id === 3), 'Disabled follow has no hit region: ' + dark);
          render(dark, 'wrist', true);
          check(follow().getAttribute('aria-pressed') === 'false', 'Stored preference is inactive in wrist mode: ' + dark);
          render(dark, 'world', false);
          const buttons = [...document.querySelectorAll('.vr-dialogue-toolbar button')];
          check(buttons.map(button => button.dataset.vrRegion).join(',') === '0,1,3,2', 'Follow is immediately before pin: ' + dark);
          check(!follow().disabled && follow().getAttribute('aria-pressed') === 'false', 'World mode exposes an off toggle: ' + dark);
          check(layout.regions.map(region => region.id).join(',') === '0,1,3,2', 'Hit regions retain stable native IDs: ' + dark);
          const bar = document.querySelector('.vr-dialogue-toolbar').getBoundingClientRect();
          check(buttons.every(button => {
            const rect = button.getBoundingClientRect();
            return rect.width >= 68 && rect.left >= bar.left && rect.right <= bar.right && rect.top >= bar.top && rect.bottom <= bar.bottom;
          }), 'Toolbar buttons fit inside the capsule: ' + dark);
          const region = layout.regions.find(region => region.id === 3);
          const rect = follow().getBoundingClientRect();
          const surface = document.querySelector('.vr-surface').getBoundingClientRect();
          check(region.circle && Math.abs(region.bounds[0] * 768 - (rect.left - surface.left)) < 0.01
            && Math.abs((region.bounds[2] - region.bounds[0]) * 768 - rect.width) < 0.01, 'Follow hit region matches the rendered button: ' + dark);
          const inactiveColor = getComputedStyle(follow()).backgroundColor;
          render(dark, 'world', true, false, 3);
          check(follow().getAttribute('aria-label') === '跟随视角', 'Follow uses its localized label: ' + dark);
          check(follow().getAttribute('aria-pressed') === 'true' && follow().classList.contains('active'), 'Enabled follow is pressed and active: ' + dark);
          check(getComputedStyle(follow()).backgroundColor !== inactiveColor, 'Enabled follow has a visible highlight: ' + dark);
          check(follow().querySelector('.vr-surface-tooltip')?.textContent === '跟随视角', 'Laser hover shows the follow tooltip: ' + dark);
          render(dark, 'world', true, true);
          check([...document.querySelectorAll('.vr-dialogue-toolbar button')].every(button => button.disabled), 'Pending save disables all placement controls: ' + dark);
          check(layout.regions.length === 0, 'Pending save has no clickable regions: ' + dark);
        }
        render(true, 'world', true);
        document.getElementById('result').textContent = JSON.stringify({ passed });
      } catch (error) {
        document.getElementById('result').textContent = JSON.stringify({ error: error.stack, passed });
      }
    ` },
    bundle: true, write: false, outfile: "follow.js", format: "esm", platform: "browser", jsx: "automatic", minify: true,
    loader: { ".png": "dataurl" },
  });
  const directory = mkdtempSync(join(testDirectory, ".vr-follow-"));
  try {
    const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
    const stylesheet = bundle.outputFiles.find(file => file.path.endsWith(".css")).text;
    const html = join(directory, "test.html");
    writeFileSync(html, '<!doctype html><html data-vr-surface><style>' + stylesheet
      + '</style><div id="root"></div><output id="result" hidden></output><script type="module">'
      + javascript.replace(/<\/script/gi, "<\\/script") + '</script></html>');
    const { stdout } = await promisify(execFile)(browser, [
      "--headless=new", "--dump-dom", "--virtual-time-budget=2000", "--disable-gpu", "--no-first-run",
      "--disable-background-networking", "--disable-extensions", "--window-size=800,860",
      ...(process.env.VR_FOLLOW_SCREENSHOT ? ["--screenshot=" + process.env.VR_FOLLOW_SCREENSHOT] : []),
      "--user-data-dir=" + join(directory, "profile"), pathToFileURL(html).href,
    ], { timeout: 35000, maxBuffer: 8 * 1024 * 1024 });
    const output = stdout.match(/<output id="result" hidden="">([^<]+)<\/output>/)?.[1];
    assert.ok(output, "The dialogue follow fixture must finish");
    const result = JSON.parse(output);
    assert.equal(result.error, undefined, result.error);
    assert.equal(result.passed.length, 28);
  } finally {
    assert.equal(dirname(directory), testDirectory);
    rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
});
