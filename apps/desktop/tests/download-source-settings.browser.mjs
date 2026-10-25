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

for (const width of [1200, 380]) {
  test(`download source selection stays compact in onboarding and general settings at ${width}px`, {
    timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
  }, async () => {
    const bundle = await build({
      stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
        import React, { useState } from 'react';
        import { createRoot } from 'react-dom/client';
        import { flushSync } from 'react-dom';
        import i18next from 'i18next';
        import { I18nextProvider } from 'react-i18next';
        import en from './src/i18n/locales/en-US.json';
        import { WelcomeStep } from './src/onboarding/steps/WelcomeStep';
        import { SystemSettingsSection } from './src/settings/sections/SystemSettingsSection';
        import './src/styles.css';
        window.requestAnimationFrame = callback => window.setTimeout(() => callback(performance.now()), 16);
        window.cancelAnimationFrame = id => window.clearTimeout(id);
        const i18n = i18next.createInstance();
        await i18n.init({ lng: 'en-US', resources: { 'en-US': en }, interpolation: { escapeValue: false } });
        const requests = [];
        const updates = [];
        const applied = [];
        window.fetch = async url => {
          requests.push(String(url));
          return Response.json({});
        };
        function Harness({ initial = 'auto', welcome = true, busy = false }) {
          const [draft, setDraft] = useState({
            features: {},
            conversation_titles: { auto_generate: false, profile_id: null, model: '', output_language: 'auto' },
            storage: { database_path: 'data', model_directory: 'models', subtitle_history_max_bytes: 10485760, model_download_source: initial },
          });
          const onChange = value => {
            updates.push(value);
            setDraft(current => ({ ...current, storage: { ...current.storage, model_download_source: value } }));
          };
          const applySettings = update => {
            const next = update(draft);
            applied.push(next);
            updates.push(next.storage.model_download_source);
            setDraft(next);
          };
          return <I18nextProvider i18n={i18n}>{welcome
            ? <WelcomeStep languagePreference="en-US" busy={busy} onUpdateLanguage={() => {}}
                downloadSource={draft.storage.model_download_source} onUpdateDownloadSource={onChange} />
            : <SystemSettingsSection desktopPreferences={{ launchAtStartup: false, runAsAdministrator: false, minimizeToTray: false }}
                desktopPreferencesReady={true} desktopSaveState="saved" desktopMessage="" uiLanguagePreference="en-US"
                interfaceScale={100} onUpdateDesktop={async () => {}} onInterfaceScaleChange={() => {}}
                onUpdateUiLanguage={async () => {}} onboardingDisabled={false} onStartOnboarding={() => {}}
                locale="en-US" draft={draft} profiles={[]} onManageProfiles={() => {}} applySettings={applySettings}
                updater={{ phase: 'idle', automaticChecks: false }} />}</I18nextProvider>;
        }
        const root = createRoot(document.getElementById('root'));
        const check = (condition, message) => { if (!condition) throw new Error(message); };
        const settle = () => new Promise(resolve => setTimeout(resolve, 20));
        const wait = async predicate => {
          for (let i = 0; i < 150; i++) { if (predicate()) return; await settle(); }
          throw new Error('Timed out: ' + predicate.toString());
        };
        const click = element => flushSync(() => element.click());
        const panel = () => document.querySelector('.download-source-settings');
        const trigger = () => panel().querySelector('.dropdown-trigger');
        const checkCompact = () => {
          check(panel().querySelectorAll('button:not([role=option])').length === 1, 'Only the download source dropdown remains');
          check(!panel().querySelector('p, h3, [role=status], [role=alert]'), 'Descriptions and detection status are removed');
          check(document.documentElement.scrollWidth <= innerWidth, 'The controls fit the viewport');
          const bounds = trigger().getBoundingClientRect();
          check(bounds.width > 0 && bounds.left >= 0 && bounds.right <= innerWidth, 'The dropdown fits the viewport');
        };
        const select = async label => {
          click(trigger());
          await wait(() => panel().querySelector('[role=option]'));
          click([...panel().querySelectorAll('[role=option]')].find(option => option.textContent === label));
          await settle();
        };
        let stage = '';
        try {
          stage = 'first launch';
          flushSync(() => root.render(<Harness />));
          await settle();
          checkCompact();
          check(trigger().textContent.includes('Automatic'), 'Automatic remains the default mode');
          check(updates.length === 0, 'Opening the selector does not overwrite a source preference');
          await select('Official source');
          check(trigger().textContent.includes('Official source'), 'The manual selection is shown');
          check(JSON.stringify(updates) === JSON.stringify(['official']), 'Only the manual selection is saved');

          stage = 'busy onboarding';
          flushSync(() => root.render(<Harness busy={true} />));
          check(trigger().disabled, 'The source selector respects onboarding busy state');

          stage = 'general settings';
          flushSync(() => root.render(<Harness key="manual" initial="modelscope" welcome={false} />));
          await settle();
          checkCompact();
          check(panel().closest('section')?.getAttribute('aria-labelledby') === 'system-general-title', 'The source selector belongs to General settings');
          check(trigger().textContent.includes('ModelScope mirror'), 'The saved source preference is displayed');
          await select('Official source');
          await select('Automatic');
          await select('ModelScope mirror');
          check(JSON.stringify(updates) === JSON.stringify(['official', 'official', 'auto', 'modelscope']), 'All source choices use the settings save callback');
          check(applied.every(next => next.storage.database_path === 'data' && next.storage.model_directory === 'models'
            && next.storage.subtitle_history_max_bytes === 10485760), 'Source changes preserve the other storage settings');
          check(!requests.some(url => url.includes('/downloads/')), 'The selector does not request removed detection controls');
          checkCompact();
          document.getElementById('result').textContent = 'PASS';
        } catch (error) { document.getElementById('result').textContent = 'FAIL: ' + stage + ': ' + error.message; }
        parent.postMessage(document.getElementById('result').textContent, '*');
      ` },
      plugins: [{ name: "locale-fixture", setup(build) {
        build.onLoad({ filter: /[\\/]i18n[\\/]catalog\.ts$/ }, () => ({
          contents: 'import en from "./src/i18n/locales/en-US.json"; export const localeCatalog = [en]; export const supportedUiLocales = ["en-US"]; export const localeResources = { "en-US": { translation: en.translation } };',
          resolveDir: join(testDirectory, ".."), loader: "ts",
        }));
      } }],
      define: { "import.meta.env": "{}" },
      bundle: true, write: false, outfile: "download-source-test.js", format: "esm", platform: "browser", jsx: "automatic",
    });
    const directory = mkdtempSync(join(testDirectory, ".download-source-browser-"));
    try {
      const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
      const stylesheet = bundle.outputFiles.find(file => file.path.endsWith(".css")).text;
      const html = join(directory, "test.html");
      writeFileSync(join(directory, "content.html"), '<style>' + stylesheet + '</style><div id="root" class="settings-surface"></div>'
        + '<pre id="result">PENDING</pre><script>const fail = message => { document.getElementById("result").textContent = "FAIL: " + message; parent.postMessage("FAIL: " + message, "*"); }; window.addEventListener("error", event => fail(event.message)); window.addEventListener("unhandledrejection", event => fail(String(event.reason)));</script>'
        + '<script type="module">' + javascript.replace(/<\/script/gi, "<\\/script") + '</script>');
      writeFileSync(html, '<body style="margin:0"><iframe src="content.html" style="border:0;width:' + width
        + 'px;height:1000px"></iframe><pre id="result">PENDING</pre><script>window.addEventListener("message", event => document.getElementById("result").textContent = event.data);</script></body>');
      const result = spawnSync(browser, [
        "--headless=new", "--dump-dom", "--virtual-time-budget=15000", "--disable-gpu", "--no-first-run",
        "--disable-background-networking", "--disable-extensions", "--window-size=" + Math.max(500, width) + ",1000",
        "--user-data-dir=" + join(directory, "profile"), pathToFileURL(html).href,
      ], { encoding: "utf8", timeout: 30000, maxBuffer: 8 * 1024 * 1024 });
      assert.ifError(result.error);
      assert.equal(result.status, 0, result.stderr);
      const report = result.stdout.match(/<pre id="result">([^<]*)<\/pre>/)?.[1];
      assert.equal(report, "PASS", report || result.stderr);
    } finally {
      assert.equal(dirname(directory), testDirectory);
      rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
    }
  });
}
