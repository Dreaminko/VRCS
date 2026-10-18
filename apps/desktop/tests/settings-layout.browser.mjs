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
  test(`settings controls align and group spacing stays compact at ${width}px`, {
    timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
  }, async () => {
    const bundle = await build({
      stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
        import React from 'react';
        import { createRoot } from 'react-dom/client';
        import i18next from 'i18next';
        import { initReactI18next } from 'react-i18next';
        import en from './src/i18n/locales/en-US.json';
        import { Select } from './src/settings/SettingsControls';
        import { LearningAiSettingsSection } from './src/settings/sections/LearningAiSettingsSection';
        import { CloudProviderSettings } from './src/settings/recognition/CloudProviderSettings';
        import { ApiProfileEditor } from './src/settings/api/ApiProfileEditor';
        import { createApiProfileDraft } from './src/providers/api-profile-draft';
        import { MicrophoneLevelSetting } from './src/settings/sections/MicrophoneLevelSetting';
        import { OcrSettingsCard } from './src/settings/sections/OcrSettingsCard';
        import { VisionSettingsCard } from './src/settings/sections/VisionSettingsCard';
        import { DEFAULT_OCR_SETTINGS } from './src/settings/vr-overlay-settings';
        import { DEFAULT_VISION_SETTINGS } from './src/settings/vision-settings';
        import './src/styles.css';
        await i18next.use(initReactI18next).init({ lng: 'en-US', resources: { 'en-US': { translation: en.translation } } });

        // Make focus callbacks advance with headless Chrome's virtual clock.
        window.requestAnimationFrame = callback => window.setTimeout(() => callback(performance.now()), 16);
        window.cancelAnimationFrame = id => window.clearTimeout(id);
        const noop = () => {};
        const profile = { id: 'sample', name: 'Sample', provider: 'sample', provider_display_name: 'Sample',
          enabled_capabilities: ['speech_to_text', 'image_understanding'] };
        window.aiFixture = { profiles: [profile], profilesLoading: false, error: '',
          selectedProfile: profile, modelDetails: {},
          preferences: { profileId: 'sample', model: 'image-model', explanationLevel: 'beginner',
            thinkingEnabled: false, reasoningControl: null },
          models: [], modelsLoading: false, modelsError: 'The model catalog could not be loaded. Check the connection and try again.',
          explanationLanguagePreference: 'ui', setProfileId: noop, setModel: noop,
          setExplanationLanguage: noop, setExplanationLevel: noop, setReasoningControl: noop };
        window.fetch = async url => String(url).endsWith('/ocr/token')
          ? new Response(JSON.stringify({ configured: true, stored_configured: true, environment_override: false }))
          : new Response(JSON.stringify({ code: 'models.unavailable', detail: 'Catalog unavailable', params: {} }), { status: 503 });
        const definition = { id: 'sample', display_name: 'Sample', category: 'cloud_provider',
          connection: { base_url: { mode: 'fixed', default: 'https://example.com/v1' },
            auth_modes: ['none'], default_auth_mode: 'none', fields: [
              { id: 'region', type: 'text', label: 'Region', default: 'local' },
              { id: 'is_local', type: 'boolean', label: 'Local service', default: false },
            ] },
          services: [{ id: 'sample_asr', display_name: 'Speech', capabilities: ['speech_to_text'],
            models: ['speech-model'], model_listing: true, supports_context: false, recognition_transport: 'realtime' }] };
        const draft = { ...createApiProfileDraft([definition]), name: 'Sample' };
        const root = createRoot(document.getElementById('root'));
        root.render(<div className="settings-section settings-section-active ocr-settings-section">
          <div id="paired" className="form-grid">
            <Select label="First field" value="sample" options={[{ value: 'sample', label: 'Sample' }]} disabled={false} onChange={noop} />
            <Select label="Second field" value="sample" options={[{ value: 'sample', label: 'Sample' }]} disabled={false}
              helper="This field has additional help text that must not move the neighboring control." onChange={noop} />
          </div>
          <LearningAiSettingsSection />
          <div id="recognition" className="recognition-config"><CloudProviderSettings
            draft={{ asr: { active_profile_id: 'sample', backend: 'sample_asr', language: 'auto', service_settings: {} } }}
            apiProfiles={[profile]} providerDefinitions={[definition]} disabled={false} onUpdateAsr={noop} onSelectService={noop} /></div>
          <div id="api"><ApiProfileEditor draft={draft} saving={false} providerDefinitions={[definition]} onChange={noop} onSave={noop} /></div>
          <MicrophoneLevelSetting level={null} enabled={true} captureRunning={false} transcriptionRunning={false}
            testing={false} busy={false} threshold={-40} disabled={false} onToggleTest={noop} onCommit={noop} />
          <div id="ocr-layout"><OcrSettingsCard config={{ ...DEFAULT_OCR_SETTINGS, targets: [] }} profiles={[]}
            disabled={false} vrAvailable={true} desktopStatus={null} onChange={noop} /></div>
          <VisionSettingsCard config={{ ...DEFAULT_VISION_SETTINGS, enabled: true, profile_id: 'sample', model: 'image-model' }}
            profiles={[profile]} disabled={false} vrAvailable={true} onChange={noop} onSelectProfile={noop} />
        </div>);
        const check = (condition, message) => { if (!condition) throw new Error(message); };
        const settle = () => new Promise(resolve => setTimeout(resolve, 20));
        const wait = async predicate => {
          for (let attempt = 0; attempt < 200; attempt++) {
            if (predicate()) return;
            await settle();
          }
          throw new Error('Timed out waiting for: ' + predicate.toString());
        };
        const rect = element => element.getBoundingClientRect();
        const sameTop = (first, second, message) => check(Math.abs(rect(first).top - rect(second).top) <= 1, message);
        try {
          await wait(() => document.querySelector('#recognition .api-model-catalog-error') && document.querySelector('.vision-settings-card .api-model-catalog-error'));
          await settle();
          check(innerWidth === ${width}, 'Expected viewport width ${width}, received ' + innerWidth);
          for (const selector of ['#paired', '.learning-ai-settings-grid']) {
            const grid = document.querySelector(selector);
            const fields = [...grid.children];
            if (getComputedStyle(grid).gridTemplateColumns.split(' ').length === 2) {
              sameTop(fields[0].querySelector('.dropdown-trigger'), fields[1].querySelector('input, .dropdown-trigger'), selector + ': help or error text does not misalign controls');
            }
          }
          const hint = document.querySelector('#recognition .api-model-catalog-error');
          const recognitionFields = hint.parentElement;
          check(Math.abs(rect(hint).left - rect(recognitionFields).left) <= 1 && Math.abs(rect(hint).right - rect(recognitionFields).right) <= 1,
            'Recognition model error spans the full field grid');
          const localControl = document.querySelector('.api-profile-local-control');
          const neighboringInput = localControl.parentElement.previousElementSibling.querySelector('input');
          check(Math.abs(rect(localControl).height - rect(neighboringInput).height) <= 1, 'API boolean control matches the input height');
          const microphone = document.querySelector('.microphone-level-setting');
          const microphoneGap = rect(microphone.querySelector('.range-field-header')).top - rect(microphone.querySelector('.microphone-test-control')).bottom;
          check(microphoneGap >= 8 && microphoneGap <= 16, 'Microphone slider does not have duplicate spacing');
          const ocrGroups = [...document.querySelectorAll('#ocr-layout .vr-overlay-card > .settings-disclosure > .ocr-settings-group')];
          for (let index = 1; index < ocrGroups.length; index++) {
            const gap = rect(ocrGroups[index].querySelector('.vr-overlay-card-heading')).top - rect(ocrGroups[index - 1]).bottom;
            check(gap >= 16 && gap <= 28, 'OCR group spacing is compact and keeps a visible section boundary');
          }
          for (const selector of ['#ocr-layout .ocr-settings-group', '.vision-settings-card .vision-settings-group']) {
            for (const group of document.querySelectorAll(selector)) {
              const heading = group.querySelector(':scope > header');
              const toggle = heading.querySelector('[role=switch]');
              if (!toggle) continue;
              const label = heading.querySelector('strong');
              check(!group.querySelector(':scope > .settings-toggle-row'), 'Mode switch does not occupy a separate row');
              check(!heading.querySelector('.settings-toggle-copy'), 'Mode heading has no redundant enable label');
              check(getComputedStyle(heading).flexDirection === 'row', 'Mode title and switch stay on the same row');
              check(rect(toggle).left >= rect(label).right && rect(toggle).right <= rect(heading).right + 1, 'Mode switch stays on the right of its title');
            }
          }
          const modelRow = document.querySelector('.vision-settings-card .translation-model-input-row');
          const model = modelRow.querySelector('.dropdown-trigger');
          const refresh = modelRow.querySelector('button.secondary-button');
          const imageTest = refresh.nextElementSibling;
          sameTop(model, refresh, 'Vision model selector and refresh button stay on one row');
          sameTop(refresh, imageTest, 'Vision image test stays on the model row');
          check(rect(model).right <= rect(refresh).left, 'Model controls do not overlap');
          check(rect(refresh).right <= rect(imageTest).left, 'Vision image test stays to the right of refresh');
          check(imageTest.textContent.trim() === '' && imageTest.getAttribute('aria-label') && imageTest.title, 'Vision image test has only an icon with an accessible label and tooltip');
          const visionGrid = document.querySelector('.vision-settings-card .vr-overlay-field-grid');
          const answerLanguage = visionGrid.querySelector('.language-picker-field .dropdown-trigger');
          if (getComputedStyle(visionGrid).gridTemplateColumns.split(' ').length === 2) {
            sameTop(visionGrid.querySelector('.dropdown-trigger'), answerLanguage, 'Vision profile and answer language remain aligned during catalog errors');
          }
          check(rect(model).top > rect(answerLanguage).bottom, 'Vision model settings stay below the answer language');
          check(Math.abs(rect(modelRow).width - rect(visionGrid.firstElementChild).width) <= 1, 'Vision model controls keep a single field width');
          check(document.documentElement.scrollWidth <= innerWidth + 1, 'Settings do not overflow horizontally');
          root.unmount();
          document.getElementById('result').textContent = 'PASS';
        } catch (error) { document.getElementById('result').textContent = 'FAIL: ' + error.message; }
      ` },
      plugins: [{ name: 'learning-state-fixture', setup(build) {
        build.onResolve({ filter: /learning\/hooks\/useLearningAiConfiguration$/ }, () => ({ path: 'learning-state', namespace: 'fixture' }));
        build.onLoad({ filter: /.*/, namespace: 'fixture' }, () => ({ contents: 'export const useLearningAiConfiguration = () => window.aiFixture;' }));
        build.onResolve({ filter: /i18n\/catalog$/ }, () => ({ path: 'locale-catalog', namespace: 'locale-fixture' }));
        build.onLoad({ filter: /.*/, namespace: 'locale-fixture' }, () => ({
          resolveDir: join(testDirectory, '..'),
          contents: 'import en from "./src/i18n/locales/en-US.json"; export const localeCatalog = [en]; export const supportedUiLocales = ["en-US"]; export const localeResources = { "en-US": { translation: en.translation } };',
        }));
      } }],
      bundle: true, write: false, outfile: 'settings-layout-test.js', format: 'esm', platform: 'browser', jsx: 'automatic',
      define: { 'import.meta.env.VITE_VRCS_SESSION_TOKEN': '""' },
    });
    const directory = mkdtempSync(join(testDirectory, '.settings-layout-browser-'));
    try {
      const html = join(directory, 'test.html');
      const content = join(directory, 'content.html');
      const javascript = bundle.outputFiles.find(file => file.path.endsWith('.js')).text;
      const stylesheet = bundle.outputFiles.find(file => file.path.endsWith('.css')).text;
      const errorCapture = '<script>const result = document.getElementById("result"); const reportError = message => { result.textContent = "FAIL: " + message; }; window.addEventListener("error", event => reportError(event.message)); window.addEventListener("unhandledrejection", event => reportError(String(event.reason))); new MutationObserver(() => parent.postMessage(result.textContent, "*")).observe(result, { childList: true });</script>';
      writeFileSync(content, '<style>' + stylesheet + '</style><div id="root" class="settings-surface"></div><pre id="result">PENDING</pre>'
        + errorCapture + '<script type="module">' + javascript.replace(/<\/script/gi, '<\\/script') + '</script>');
      // An iframe sets the CSS viewport exactly, without the browser window's minimum-width limit.
      writeFileSync(html, '<iframe src="content.html" style="display:block;border:0;width:' + width + 'px;height:900px"></iframe><pre id="result">PENDING</pre>'
        + '<script>window.addEventListener("message", event => { if (event.source === document.querySelector("iframe").contentWindow) document.getElementById("result").textContent = event.data; });</script>');
      const result = spawnSync(browser, [
        '--headless=new', '--dump-dom', '--virtual-time-budget=10000', '--disable-gpu', '--no-first-run',
        '--disable-background-networking', '--disable-extensions', '--window-size=' + width + ',900',
        '--user-data-dir=' + join(directory, 'profile'), pathToFileURL(html).href,
      ], { encoding: 'utf8', timeout: 30000, maxBuffer: 8 * 1024 * 1024 });
      assert.ifError(result.error);
      assert.equal(result.status, 0, result.stderr);
      const report = result.stdout.match(/<pre id="result">([^<]*)<\/pre>/)?.[1];
      assert.equal(report, 'PASS', report || result.stderr);
    } finally {
      rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
    }
  });
}
