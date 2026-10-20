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

test("settings follow the configured translation, recognition, network and VR services", {
  timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
}, async () => {
  const bundle = await build({
    stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
      import React, { useState } from 'react';
      import { createRoot } from 'react-dom/client';
      import i18next from 'i18next';
      import { initReactI18next } from 'react-i18next';
      import { TranslationSettingsSection } from './src/settings/sections/TranslationSettingsSection';
      import { ApiManagementSettingsSection } from './src/settings/sections/ApiManagementSettingsSection';
      import { OcrSettingsSection } from './src/settings/sections/OcrSettingsSection';
      import { ExternalApiSettingsCard } from './src/settings/system/ExternalApiSettingsCard';
      import { VadSettings } from './src/settings/recognition/VadSettings';
      import { useAsrModels } from './src/settings/hooks/useAsrModels';
      import { DEFAULT_OCR_SETTINGS } from './src/settings/vr-overlay-settings';
      import { DEFAULT_VISION_SETTINGS } from './src/settings/vision-settings';
      await i18next.use(initReactI18next).init({ lng: 'en', resources: { en: { translation: {} } } });
      const profile = (id, provider, llm, local = false) => ({
        id, name: id, provider, provider_display_name: provider, is_local: local,
        enabled_capabilities: id === 'asr' ? ['speech_to_text'] : llm ? ['text_generation', 'text_translation'] : ['text_translation'],
        credential: { configured: true }, support_levels: {},
        capabilities: { supports_model_listing: llm, supports_text_generation: llm, supports_context: llm,
          supports_asr: id === 'asr', supports_translation: id !== 'asr', is_local: local,
          supported_languages: ['en', 'ja', 'zh-Hans'], supports_custom_translation_language: true },
      });
      const profiles = [profile('deepseek', 'deepseek', true), profile('deepl', 'deepl', false),
        profile('local', 'openai_compatible', true, true), profile('asr', 'alibaba_cloud', false)];
      const services = ['fun_asr_realtime', 'qwen_realtime'].map(id => ({
        id, display_name: id, capabilities: ['speech_to_text'], models: ['asr-model'],
      }));
      const definitions = profiles.map(p => ({ id: p.provider, display_name: p.provider,
        connection: { base_url: { mode: 'fixed' }, fields: [] }, services: p.id === 'asr' ? services : [] }));
      let failCatalogs = false;
      const diagnostics = [];
      window.fetch = async url => {
        const path = String(url);
        let data = { configured: true, state: 'ready' };
        if (path.endsWith('/api/asr/profiles')) data = { profiles };
        else if (path.endsWith('/api/providers')) data = { providers: definitions };
        else if (path.endsWith('/models')) {
          if (failCatalogs) throw new Error('Catalog unavailable');
          data = { models: ['deepseek-v4-flash'] };
        } else if (path.endsWith('/api/asr/local-models/qwen')) data = [];
        else if (path.includes('/test?')) { diagnostics.push(path); data = { ok: true, checks: [] }; }
        return new Response(JSON.stringify(data), { status: 200 });
      };
      const target = (id, language = 'ja', model = 'deepseek-v4-flash') => ({
        target_language: language, profile_id: id, model, thinking_enabled: false,
      });
      const initial = {
        asr: { backend: 'qwen_realtime', active_profile_id: 'asr', service_settings: {}, managed_qwen: {} },
        vad: { silence_seconds: 0.1, max_speech_seconds: 10, endpointing: 'silence' },
        storage: { model_directory: 'models' }, language_presets: [],
        features: { vr_overlay: true, ocr: false, visual_question_answering: true },
        ocr: { ...DEFAULT_OCR_SETTINGS, hand_gesture_enabled: true, targets: [target('deepseek')] },
        vision: { ...DEFAULT_VISION_SETTINGS, enabled: true, vr_enabled: true },
        translation: { mode: 'manual', speaker_targets: [target('deepl')], microphone_targets: [target('deepseek')],
          prompt: { system_prompt: 'Translate.{context}{glossary}', context_enabled: true,
            include_speaker: true, include_microphone: true, include_chatbox: true, max_messages: 2, max_chars: 1000 } },
        external_api: { enabled: true, host: '127.0.0.1', port: 9003, require_token: false },
      };
      const noop = async () => {};
      let draft, setDraft, asrActions;
      function Harness() {
        [draft, setDraft] = useState(initial);
        asrActions = useAsrModels({ active: false, settings: draft, onModelsChanged: noop,
          draftController: { applySettings: setDraft }, apiProfiles: profiles, providerDefinitions: definitions });
        return <>
          <TranslationSettingsSection draft={draft} apiProfiles={profiles} applySettings={setDraft} />
          <VadSettings vad={draft.vad} backend={draft.asr.backend} disabled={false} onUpdate={asrActions.updateVad} />
          <ApiManagementSettingsSection settings={draft} onRefreshSettings={noop} />
          <ExternalApiSettingsCard config={draft.external_api} saveState="saved"
            onChange={patch => setDraft(current => ({ ...current, external_api: { ...current.external_api, ...patch } }))} />
          <OcrSettingsSection vrAvailable={true} draft={draft} profiles={profiles} applySettings={setDraft} />
        </>;
      }
      const root = createRoot(document.getElementById('root'));
      root.render(<Harness />);
      const check = (condition, message) => { if (!condition) throw new Error(message); };
      const settle = () => new Promise(resolve => setTimeout(resolve, 20));
      const wait = async predicate => {
        for (let attempt = 0; attempt < 200; attempt++) {
          if (predicate()) { await settle(); return; }
          await settle();
        }
        throw new Error('Timed out: ' + predicate.toString());
      };
      const update = async patch => { setDraft(current => ({ ...current, ...patch })); await settle(); };
      const enhancement = () => document.querySelector('.translation-enhancement-row');
      const addRoute = () => document.querySelector('.translation-route-add').click();
      const setRoutes = async (microphone_targets, speaker_targets = [target('deepl')], mode = 'manual', backend = 'qwen_realtime') => {
        await update({ asr: { ...draft.asr, backend }, translation: { ...draft.translation, mode, microphone_targets, speaker_targets } });
      };
      try {
        await wait(() => document.querySelectorAll('.api-profile-row').length === 4);
        check(enhancement(), 'A microphone LLM route exposes global enhancement even when the speaker uses DeepL');
        failCatalogs = true;
        addRoute();
        await wait(() => draft.translation.microphone_targets.length === 2);
        check(draft.translation.microphone_targets[1].profile_id === 'deepseek'
          && draft.translation.microphone_targets[1].model === 'deepseek-v4-flash', 'Added route keeps a matching provider and model without a catalog');
        await setRoutes([target('local', 'ja', 'custom-local-model')]);
        addRoute();
        await wait(() => draft.translation.microphone_targets.length === 2);
        check(draft.translation.microphone_targets[1].model === 'custom-local-model', 'Compatible providers retain manually configured models');
        check(document.querySelector('.translation-privacy-notice').textContent.includes('contextPrivacyLocal'), 'Local routes show local privacy scope');
        await setRoutes([target('local')], [target('deepseek')]);
        check(document.querySelector('.translation-privacy-notice').textContent.includes('contextPrivacyCloud'), 'A cloud route prevents a local-only privacy claim');
        const native = target(null, 'ja', '');
        await setRoutes([native], [native], 'automatic', 'qwen_live_translate');
        check(!enhancement(), 'Native Live routes do not expose LLM enhancement');
        addRoute();
        await wait(() => document.querySelector('.translation-route-group > [role=alert]'));
        check(draft.translation.microphone_targets.length === 1, 'Catalog failures never save an empty LLM route');
        failCatalogs = false;
        addRoute();
        await wait(() => draft.translation.microphone_targets.length === 2);
        check(draft.translation.microphone_targets[1].model === 'deepseek-v4-flash', 'An unconfigured route selects a text model from its provider catalog');
        check(enhancement(), 'Extra LLM routes expose enhancement alongside native Live translation');
        await setRoutes([target('deepseek')], [target('deepl')], 'manual', 'qwen_live_translate');
        check(enhancement(), 'Manual translation exposes enhancement with a Live ASR backend');

        await update({ asr: { ...draft.asr, backend: 'qwen_realtime' } });
        asrActions.updateRecognitionService('fun_asr_realtime');
        await wait(() => draft.asr.backend === 'fun_asr_realtime');
        check(draft.vad.silence_seconds === 0.2, 'Service selection raises the silence threshold to the Fun-ASR minimum');
        check(document.querySelector('input[type=range]').min === '0.2', 'The Fun-ASR slider cannot select an invalid threshold');
        await update({ asr: { ...draft.asr, backend: 'qwen_local_managed', active_profile_id: null }, vad: { ...draft.vad, silence_seconds: 0.1 } });
        asrActions.updateRecognitionSource('asr');
        await wait(() => draft.asr.backend === 'fun_asr_realtime');
        check(draft.vad.silence_seconds === 0.2, 'Profile selection also raises the Fun-ASR minimum');
        await update({ vad: { ...draft.vad, silence_seconds: 0.5 } });
        asrActions.updateRecognitionService('qwen_realtime');
        await wait(() => draft.asr.backend === 'qwen_realtime');
        check(draft.vad.silence_seconds === 0.5 && document.querySelector('input[type=range]').min === '0.1', 'Switching services preserves valid user thresholds');
        [...document.querySelectorAll('.api-profile-row[aria-label=asr] button')]
          .find(button => button.textContent.includes('testAsr')).click();
        await wait(() => diagnostics.length > 0);
        check(new URL(diagnostics[0]).searchParams.get('service_id') === 'qwen_realtime', 'ASR diagnostics test the selected service instead of the first service');

        root.unmount();
        document.getElementById('result').textContent = 'PASS';
      } catch (error) { document.getElementById('result').textContent = 'FAIL: ' + error.message; }
    ` },
    bundle: true, write: false, format: "esm", platform: "browser", jsx: "automatic",
    define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
  });
  const directory = mkdtempSync(join(testDirectory, ".settings-alignment-browser-"));
  try {
    const html = join(directory, "test.html");
    writeFileSync(html, '<div id="root"></div><pre id="result">PENDING</pre><script type="module">'
      + bundle.outputFiles[0].text.replace(/<\/script/gi, "<\\/script") + '</script>');
    const result = spawnSync(browser, [
      "--headless=new", "--dump-dom", "--virtual-time-budget=15000", "--disable-gpu", "--no-first-run",
      "--disable-background-networking", "--disable-extensions", "--user-data-dir=" + join(directory, "profile"),
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
