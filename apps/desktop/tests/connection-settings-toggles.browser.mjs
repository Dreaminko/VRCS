import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const testDirectory = dirname(fileURLToPath(import.meta.url));
const browser = [process.env.PROGRAMFILES, process.env["PROGRAMFILES(X86)"], process.env.LOCALAPPDATA]
  .filter(Boolean).flatMap(root => [
    join(root, "Google/Chrome/Application/chrome.exe"),
    join(root, "Microsoft/Edge/Application/msedge.exe"),
  ]).find(existsSync);

for (const width of [1200, 500]) {
test(`connection heading switches hide dependent controls and retain configuration at ${width}px`, {
  timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
}, async () => {
  const bundle = await build({
    stdin: {
      resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
        import React, { useState } from 'react';
        import { createRoot } from 'react-dom/client';
        import i18next from 'i18next';
        import { initReactI18next } from 'react-i18next';
        import './src/styles.css';
        import en from './src/i18n/locales/en-US.json';
        import { ConnectionSettingsSection } from './src/settings/sections/ConnectionSettingsSection';
        await i18next.use(initReactI18next).init({ lng: 'en-US', resources: { 'en-US': { translation: en.translation } } });
        window.fetch = async () => new Response(JSON.stringify({
          configured: true, stored_configured: true, environment_override: false, state: 'disabled',
        }), { status: 200 });
        const initial = {
          osc: { enabled: false, port: 9001, translation_strategy: 'round_robin',
            preserve_original_text: true, mute_sync_enabled: true, mute_status_toast_enabled: false },
          anki: { enabled: false, port: 8766, deck: 'Saved deck', model: 'Basic', front_field: 'Front', back_field: 'Back' },
          vrcx: { enabled: false, port: 9002, include_in_llm_context: true, include_in_asr_context: false },
          external_api: { enabled: false, host: '127.0.0.1', port: 9003, require_token: true },
        };
        const noop = () => {};
        let settings;
        function Harness() {
          const [draft, setDraft] = useState(initial);
          settings = draft;
          return <ConnectionSettingsSection draft={draft}
            features={{ osc_chatbox: true, anki: true, vrcx: true, external_api: true }}
            health={null} saveState="saved" applySettings={setDraft} onTest={async () => {}}
            anki={{ status: null, busy: false, message: '', portText: '8766', portError: '',
              deckNames: [], modelOptions: [], frontFieldOptions: [], backFieldOptions: [],
              onLoadStatus: async () => {}, onSetPortText: noop, onCommitPort: noop,
              onUpdate: (key, value) => setDraft(current => ({ ...current, anki: { ...current.anki, [key]: value } })),
            }} />;
        }
        const root = createRoot(document.getElementById('root'));
        root.render(<Harness />);
        const check = (condition, message) => { if (!condition) throw new Error(message); };
        const settle = () => new Promise(resolve => setTimeout(resolve, 20));
        const wait = async predicate => {
          for (let attempt = 0; attempt < 200; attempt++) {
            for (const animation of document.getAnimations()) {
              if (animation.effect.getTiming().iterations !== Infinity) animation.finish();
            }
            if (predicate()) { await settle(); return; }
            await settle();
          }
          throw new Error('Timed out waiting for the frontend');
        };
        try {
          await wait(() => document.querySelector('#connections-external-api-title'));
          const cases = [
            { key: 'osc', selector: '.osc-section', fields: 1, switches: 4, operations: 1 },
            { key: 'anki', selector: '.anki-section', fields: 1, switches: 1, operations: 1 },
            { key: 'vrcx', selector: '.vrcx-settings', fields: 2, switches: 3, operations: 1 },
            { key: 'external_api', selector: '.external-api-settings:not(.vrcx-settings)', fields: 3, switches: 2, operations: 0 },
          ];
          for (const item of cases) {
            const section = document.querySelector(item.selector);
            const master = section.querySelector('[role=switch]');
            const checkHeading = () => {
              const heading = section.querySelector(':scope > .section-heading');
              check(heading.contains(master), item.key + ': master switch is in the heading');
              check(!heading.querySelector('.settings-toggle-copy'), item.key + ': no visible enable label remains');
              check(master.getAttribute('aria-label'), item.key + ': switch retains its accessible label');
              if (item.key === 'osc') check(master.getAttribute('aria-label') === 'Enable OSC', 'OSC switch identifies the master feature, not only chatbox output');
              const label = heading.querySelector('h3').getBoundingClientRect();
              const control = master.getBoundingClientRect();
              check(control.left >= label.right && control.right <= heading.getBoundingClientRect().right + 1, item.key + ': switch stays on the right');
              check(getComputedStyle(section).borderTopWidth === '1px', item.key + ': section uses a single card frame');
              check(getComputedStyle(section).paddingTop === '16px', item.key + ': card padding matches vision settings');
            };
            const collapsed = () => {
              checkHeading();
              check(section.querySelector('h3'), item.key + ': title remains visible');
              check(section.querySelectorAll('[role=switch]').length === 1, item.key + ': only the master switch remains');
              check(!master.disabled && master.getAttribute('aria-checked') === 'false', item.key + ': master remains available');
              check(!section.querySelector('input, [role=combobox], [role=status], .section-heading button:not([role=switch])'), item.key + ': dependent controls and status are hidden');
              check(getComputedStyle(master.closest('.settings-toggle-row')).borderBottomWidth === '0px', item.key + ': no divider remains below the switch');
              const toggleList = section.querySelector('.settings-toggle-list');
              if (toggleList) check(getComputedStyle(toggleList).borderBottomWidth === '0px', item.key + ': no divider remains on the switch list');
              check(!section.querySelector('hr, [role=separator], .anki-mapping-grid, .anki-panel, .osc-panel, .external-api-panel'), item.key + ': no hidden-content separator or empty panel remains');
            };
            collapsed();
            for (let cycle = 0; cycle < 2; cycle++) {
              master.click();
              await wait(() => section.querySelector('input'));
              check(master.getAttribute('aria-checked') === 'true', item.key + ': master is enabled');
              checkHeading();
              const panel = section.querySelector('.anki-panel, .osc-panel, .external-api-panel');
              check(getComputedStyle(panel).borderTopWidth === '0px', item.key + ': no duplicate inner card frame remains');
              check(section.querySelectorAll('input').length === item.fields, item.key + ': fields are restored');
              check(section.querySelectorAll('[role=switch]').length === item.switches, item.key + ': sub-switches are restored');
              check(section.querySelectorAll('.section-heading button:not([role=switch])').length === item.operations, item.key + ': operations are restored');
              check(section.querySelector('[aria-live=polite]'), item.key + ': status is restored');
              check(section.querySelector('input[inputmode=numeric], input[type=number]').value === String(initial[item.key].port), item.key + ': port is retained');
              master.click();
              await wait(() => !section.querySelector('input'));
              collapsed();
              check(JSON.stringify(settings[item.key]) === JSON.stringify(initial[item.key]), item.key + ': configuration is retained');
            }
          }
          root.unmount();
          document.getElementById('result').textContent = 'PASS';
        } catch (error) { document.getElementById('result').textContent = 'FAIL: ' + error.message; }
      `,
    },
    bundle: true, write: false, outfile: "connection-settings-test.js", format: "esm", platform: "browser", jsx: "automatic",
    define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
  });
  const directory = mkdtempSync(join(testDirectory, ".connection-settings-browser-"));
  try {
    const html = join(directory, "test.html");
    const javascript = bundle.outputFiles.find(file => file.path.endsWith('.js')).text;
    const stylesheet = bundle.outputFiles.find(file => file.path.endsWith('.css')).text;
    writeFileSync(html, '<style>' + stylesheet + '</style><div id="root" class="settings-surface"></div><pre id="result">PENDING</pre><script type="module">'
      + javascript.replace(/<\/script/gi, "<\\/script") + '</script>');
    const result = spawnSync(browser, [
      "--headless=new", "--dump-dom", "--virtual-time-budget=10000", "--disable-gpu", "--no-first-run",
      "--disable-background-networking", "--disable-extensions", "--window-size=" + width + ",900", "--user-data-dir=" + join(directory, "profile"),
      pathToFileURL(html).href,
    ], { encoding: "utf8", timeout: 30000, maxBuffer: 8 * 1024 * 1024 });
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    const report = result.stdout.match(/<pre id="result">([^<]*)<\/pre>/)?.[1];
    assert.equal(report, "PASS", report || result.stderr);
  } finally {
    rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
});
}
