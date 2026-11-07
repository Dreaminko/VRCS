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

test("autosave keeps settings visually stable, queues edits, and rolls back failures", {
  timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
}, async () => {
  const bundle = await build({
    stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
      import React, { useState } from 'react';
      import { createRoot } from 'react-dom/client';
      import i18next from 'i18next';
      import { initReactI18next } from 'react-i18next';
      import { TranslationSettingsSection } from './src/settings/sections/TranslationSettingsSection';
      import { GlossarySettingsSection } from './src/settings/sections/GlossarySettingsSection';
      import { MicrophoneLevelSetting } from './src/settings/sections/MicrophoneLevelSetting';
      import { SystemSettingsSection } from './src/settings/sections/SystemSettingsSection';
      import { PreferenceToggle } from './src/settings/SettingsControls';
      import { useDesktopPreferences } from './src/settings/hooks/useDesktopPreferences';
      import { useSettingsDraft } from './src/settings/hooks/useSettingsDraft';
      import { createSettingsAutosave } from './src/settings/settings-autosave';
      import './src/styles.css';
      await i18next.use(initReactI18next).init({ lng: 'en', resources: { en: { translation: {} } } });
      localStorage.setItem('vrcs.theme', 'light');
      document.documentElement.dataset.theme = 'light';
      window.fetch = async () => new Response('[]', { status: 200 });
      const target = { target_language: 'ja', profile_id: 'p1', model: 'test-model', thinking_enabled: false };
      const profile = { id: 'p1', name: 'Test', provider: 'openai_compatible', provider_display_name: 'Test',
        enabled_capabilities: ['text_translation'], capabilities: { supported_languages: ['en', 'ja'] } };
      const initial = {
        asr: { backend: 'qwen_local_managed' },
        translation: { mode: 'manual', microphone_targets: [target], speaker_targets: [target] },
        language_presets: [], glossary: { llm_enabled: false, asr_enabled: false, sources: [] },
        features: {}, conversation_titles: { auto_generate: false, profile_id: null, model: '', output_language: 'auto' },
        storage: { subtitle_history_max_bytes: 10485760, model_download_source: 'auto' },
      };
      const pending = [];
      let persisted = initial;
      let state;
      let desktop;
      function Harness() {
        const [settings, setSettings] = useState(initial);
        const [save] = useState(() => createSettingsAutosave({
          persist: next => new Promise((resolve, reject) => pending.push({ next,
            resolve: () => { persisted = next; resolve(next); }, reject,
          })),
          onOptimistic: setSettings, onCommit: setSettings,
          onError: () => setSettings(persisted),
        }));
        const controller = useSettingsDraft(settings, save);
        state = controller;
        desktop = useDesktopPreferences();
        return <section className="settings-surface">
          <SystemSettingsSection desktopPreferences={desktop.desktopPreferences} desktopPreferencesReady={desktop.ready}
            uiLanguageSaving={desktop.uiLanguageSaving}
            desktopMessage={desktop.message} uiLanguagePreference={desktop.uiLanguagePreference}
            interfaceScale={100} onUpdateDesktop={desktop.updateDesktop} onInterfaceScaleChange={() => {}}
            onUpdateUiLanguage={desktop.updateUiLanguage} onboardingDisabled={false} onStartOnboarding={() => {}}
            locale="en-US" draft={controller.draft} profiles={[]} onManageProfiles={() => {}} debugRows={[]}
            applySettings={controller.applySettings} updater={{ phase: 'idle', automaticChecks: false }} />
          <TranslationSettingsSection {...controller} apiProfiles={[profile]} onManageLocalModels={() => {}} />
          <GlossarySettingsSection {...controller} />
          <MicrophoneLevelSetting level={null} enabled={true} captureRunning={false} transcriptionRunning={false}
            testing={false} busy={false} threshold={-40} disabled={false} testDisabled={controller.saveState === 'saving'}
            onToggleTest={() => {}} onCommit={() => {}} />
          <div id="unavailable"><PreferenceToggle title="Unavailable" checked={false} disabled={true} onChange={() => {}} /></div>
        </section>;
      }
      const root = createRoot(document.getElementById('root'));
      root.render(<Harness />);
      const check = (condition, message) => { if (!condition) throw new Error(message); };
      const settle = () => new Promise(resolve => setTimeout(resolve, 20));
      const wait = async predicate => {
        for (let attempt = 0; attempt < 200; attempt++) {
          if (predicate()) return;
          await settle();
        }
        throw new Error('Timed out waiting for the frontend');
      };
      try {
        await wait(() => desktop?.ready && document.querySelectorAll('.glossary-usage-toggles button').length === 2);
          check(!document.querySelector('#settings-panel-translation > .section-heading > .secondary-button'),
            'Translation settings do not expose a local model manager action');
        const switches = [...document.querySelectorAll('.glossary-usage-toggles button')];
        const controls = [...document.querySelectorAll('.dropdown-trigger, .range-input, .settings-toggle-row button')];
        const snapshot = () => controls.map(el => {
          const style = getComputedStyle(el);
          return [el.disabled, style.color, style.backgroundColor, style.opacity,
            getComputedStyle(el.closest('.settings-toggle-row') ?? el).opacity];
        });
        const baseline = JSON.stringify(snapshot());
        const stable = () => {
          check(controls.every(el => el.isConnected), 'Settings controls remain mounted');
          check(JSON.stringify(snapshot()) === baseline, 'Saving does not change control availability, color or opacity');
          check(document.querySelector('#unavailable button').disabled, 'Unavailable settings remain disabled');
          check(getComputedStyle(document.querySelector('#unavailable .settings-toggle-row')).opacity === '0.62', 'Unavailable settings keep their disabled appearance');
        };
        switches[0].focus();
        switches[0].click();
        await wait(() => state.saveState === 'saving');
        stable();
        check(document.activeElement === switches[0], 'Saving does not remove focus');
        check(document.querySelector('.microphone-test-button').disabled, 'Microphone testing stays locked until settings are saved');
        switches[1].click();
        await settle();
        switches[0].click();
        await settle();
        stable();
        check(pending.length === 1, 'Only one save request runs at a time');
        pending[0].resolve();
        await wait(() => pending.length === 2);
        check(!pending[1].next.glossary.llm_enabled && pending[1].next.glossary.asr_enabled, 'Queued save contains the latest combined changes');
        stable();
        pending[1].resolve();
        await wait(() => state.saveState === 'saved');
        stable();
        check(!state.draft.glossary.llm_enabled && state.draft.glossary.asr_enabled, 'Earlier responses do not overwrite newer edits');
        check(!document.querySelector('.microphone-test-button').disabled, 'Microphone testing becomes available after saving');
        switches[0].click();
        await wait(() => pending.length === 3);
        stable();
        pending[2].reject(new Error('Save failed'));
        await wait(() => state.saveState === 'error');
        check(!state.draft.glossary.llm_enabled && state.draft.glossary.asr_enabled, 'Failed saves restore persisted settings');
        stable();
        const [startup, administrator, tray] = [...document.querySelectorAll('[aria-labelledby=system-general-title] [role=switch]')].slice(1);
        startup.click();
        await wait(() => window.desktopPending.length === 1);
        stable();
        administrator.click();
        await settle();
        tray.click();
        await settle();
        check(startup.getAttribute('aria-checked') === 'true' && administrator.getAttribute('aria-checked') === 'true'
          && tray.getAttribute('aria-checked') === 'false', 'Desktop edits update immediately while earlier saves are pending');
        check(window.desktopPending.length === 1, 'Desktop writes run one at a time');
        window.desktopPending[0].resolve();
        await wait(() => window.desktopPending.length === 2);
        check(administrator.getAttribute('aria-checked') === 'true' && tray.getAttribute('aria-checked') === 'false', 'Earlier desktop responses preserve newer edits');
        window.desktopPending[1].reject(new Error('Desktop save failed'));
        await wait(() => window.desktopPending.length === 3);
        check(startup.getAttribute('aria-checked') === 'true' && administrator.getAttribute('aria-checked') === 'false'
          && tray.getAttribute('aria-checked') === 'false', 'A failed desktop save only rolls back its own setting');
        check(document.querySelector('[aria-labelledby=system-general-title] [role=alert]')?.textContent === 'Desktop save failed', 'Desktop save errors stay visible');
        window.desktopPending[2].resolve();
        await settle();
        stable();
        startup.click();
        await wait(() => window.desktopPending.length === 4);
        startup.click();
        await settle();
        window.desktopPending[3].reject(new Error('Superseded save failed'));
        await wait(() => window.desktopPending.length === 5);
        check(startup.getAttribute('aria-checked') === 'true', 'A superseded failure does not roll back the latest choice');
        window.desktopPending[4].resolve();
        await settle();
        check(desktop.desktopPreferences.launchAtStartup && !desktop.desktopPreferences.runAsAdministrator
          && !desktop.desktopPreferences.minimizeToTray, 'Confirmed desktop choices retain changes across settings');
        stable();
        root.unmount();
        document.getElementById('result').textContent = 'PASS';
      } catch (error) { document.getElementById('result').textContent = 'FAIL: ' + error.message; }
    ` },
    plugins: [{ name: 'desktop-fixture', setup(build) {
      build.onResolve({ filter: /shell\/desktop-preferences$/ }, () => ({ path: 'desktop', namespace: 'fixture' }));
      build.onResolve({ filter: /(?:i18n\/catalog|^\.\/catalog)$/ }, () => ({ path: 'locale', namespace: 'fixture' }));
      build.onLoad({ filter: /.*/, namespace: 'fixture' }, ({ path }) => path === 'locale' ? {
        resolveDir: join(testDirectory, '..'),
        contents: 'import en from "./src/i18n/locales/en-US.json"; export const localeCatalog=[en]; export const supportedUiLocales=["en-US"]; export const localeResources={"en-US":{translation:en.translation}};',
      } : { contents: `
        export const defaultDesktopPreferences = { launchAtStartup: false, runAsAdministrator: false, minimizeToTray: true };
        export const loadDesktopPreferences = async () => defaultDesktopPreferences;
        window.desktopPending = [];
        export const updateDesktopPreference = (current, key, enabled) => new Promise((resolve, reject) => {
          window.desktopPending.push({ resolve: () => resolve({ ...current, [key]: enabled }), reject });
        });
      ` });
    } }],
    bundle: true, write: false, outfile: "settings-autosave-test.js", format: "esm", platform: "browser", jsx: "automatic",
    define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
  });
  const directory = mkdtempSync(join(testDirectory, ".settings-autosave-browser-"));
  try {
    const html = join(directory, "test.html");
    const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
    const stylesheet = bundle.outputFiles.find(file => file.path.endsWith(".css")).text;
    writeFileSync(html, '<style>' + stylesheet + '</style><div id="root"></div><pre id="result">PENDING</pre><script type="module">'
      + javascript.replace(/<\/script/gi, "<\\/script") + '</script>');
    const result = spawnSync(browser, [
      "--headless=new", "--no-sandbox", "--dump-dom", "--virtual-time-budget=10000", "--disable-gpu", "--no-first-run",
      "--disable-background-networking", "--disable-extensions", "--user-data-dir=" + join(directory, "profile"),
      pathToFileURL(html).href,
    ], { encoding: "utf8", timeout: 30000, maxBuffer: 8 * 1024 * 1024 });
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    const report = result.stdout.match(/<pre id="result">([^<]*)<\/pre>/)?.[1];
    assert.equal(report, "PASS", report || result.stderr);
  } finally { rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }); }
});
