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
      import { useApiProfileViews } from './src/settings/hooks/useApiProfileViews';
      import { DEFAULT_OCR_SETTINGS } from './src/settings/vr-overlay-settings';
      import { DEFAULT_VISION_SETTINGS } from './src/settings/vision-settings';
      import { WindowsCapabilities } from './tests/fixtures/platform-capabilities';
      await i18next.use(initReactI18next).init({ lng: 'en', resources: { en: { translation: {
        settings: { apiManagement: { confirmDelete: 'Delete {{name}}? {{impact}}', affectedFeatures: 'Affected: {{features}}', usedBy: 'Linked: {{features}}' } },
      } } } });
      const profile = (id, provider, llm, local = false) => ({
        id, name: id, provider, provider_display_name: provider, is_local: local,
        enabled_capabilities: id === 'asr' ? ['speech_to_text'] : llm ? ['text_generation', 'text_translation'] : ['text_translation'],
        credential: { configured: true, stored_configured: true, environment_override: false, storage_available: false, reason_code: 'credentials.locked', source: 'credential_manager' }, support_levels: {},
        usages: id === 'deepseek' ? [
          { kind: 'translation', capability: 'text_translation', scope: 'microphone', index: 0, target_language: 'ja' },
          { kind: 'language_preset', capability: 'text_translation', scope: 'speaker', index: 1,
            target_language: 'en', preset_id: 'meeting', preset_name: 'Meeting' },
          { kind: 'conversation_titles', capability: 'text_generation' },
        ] : [],
        capabilities: { supports_model_listing: llm, supports_text_generation: llm, supports_context: llm,
          supports_asr: id === 'asr', supports_translation: id !== 'asr', is_local: local,
          supported_languages: ['en', 'ja', 'zh-Hans'], supports_custom_translation_language: true },
      });
      const profiles = [profile('deepseek', 'deepseek', true), profile('deepl', 'deepl', false),
        profile('local', 'openai_compatible', true, true), profile('asr', 'alibaba_cloud', false)];
      for (const [id, capability] of [['text', 'text_generation'], ['image', 'image_understanding']]) {
        const item = profile(id, 'openai_compatible', true, true);
        item.enabled_capabilities = [capability];
        item.capabilities.supports_translation = false;
        item.capabilities.supports_text_generation = capability === 'text_generation';
        item.capabilities.supports_image_understanding = capability === 'image_understanding';
        profiles.push(item);
      }
      const services = ['fun_asr_realtime', 'qwen_realtime'].map(id => ({
        id, display_name: id, capabilities: ['speech_to_text'], models: ['asr-model'],
      }));
      const definitions = profiles.map(p => ({ id: p.provider, display_name: p.provider,
        category: 'cloud_provider',
        connection: { base_url: { mode: 'fixed' }, fields: [], auth_modes: ['bearer'], default_auth_mode: 'bearer' },
        services: p.id === 'asr' ? services : [{ id: p.provider + '-chat',
          display_name: p.provider, capabilities: [...new Set(profiles.filter(item => item.provider === p.provider)
            .flatMap(item => item.enabled_capabilities))], models: [] }] }));
      let failCatalogs = false, failSave = true, failProfileReads = false, failSettingsRefresh = false, failDiagnostic = false;
      let profileReads = 0, providerReads = 0, deferTest = false, cancelledTests = 0, profileWrites = 0, deletionPreviews = 0;
      const diagnostics = [];
      window.fetch = async (url, init = {}) => {
        const requestUrl = new URL(String(url)); const path = requestUrl.pathname;
        if (requestUrl.searchParams.has('dry_run')) {
          if (init.method === 'DELETE') deletionPreviews++;
          return new Response(JSON.stringify({ side_effects: [
            { kind: 'conversation_titles', capability: 'text_generation' },
          ] }), { status: 200 });
        }
        if (init.method === 'PUT' && path.endsWith('/api/profile/deepseek')) {
          profileWrites++;
          if (failSave) return new Response(JSON.stringify({ code: 'profile.invalid', detail: 'Fixture save error', field: 'name' }), { status: 422 });
          const saved = Object.assign(profiles.find(item => item.id === 'deepseek'), JSON.parse(init.body));
          return new Response(JSON.stringify(saved), { status: 200 });
        }
        if (init.method === 'DELETE' && path.endsWith('/api/profile/text')) {
          profiles.splice(profiles.findIndex(item => item.id === 'text'), 1);
          return new Response(JSON.stringify({ deleted: true }), { status: 200 });
        }
        let data = { configured: true, state: 'ready' };
        if (path.endsWith('/api/profile')) {
          profileReads++;
          if (failProfileReads) return new Response(JSON.stringify({ detail: 'Fixture catalog refresh error' }), { status: 500 });
          data = { profiles };
        }
        else if (path.endsWith('/api/providers')) { providerReads++; data = { providers: definitions }; }
        else if (path.endsWith('/models') && requestUrl.searchParams.has('cached_only')) data = { models: ['chat-fixture', 'image-fixture'] };
        else if (path.endsWith('/models')) {
          if (failCatalogs) throw new Error('Catalog unavailable');
          data = { models: ['deepseek-v4-flash'] };
        } else if (path.endsWith('/api/asr/local-models/qwen')) data = [];
        else if (path.endsWith('/test')) {
          diagnostics.push(String(url));
          if (deferTest) return new Promise((resolve, reject) => {
            init.signal.addEventListener('abort', () => { cancelledTests++; reject(init.signal.reason); }, { once: true });
          });
          data = { ok: true, latency_ms: 617, checks: [
            { name: 'configuration', status: 'passed' }, { name: 'completion', status: 'passed' },
          ] };
          if (failDiagnostic) data = { ok: false, checks: [
            { name: 'completion', status: 'failed', code: 'llm.test_model_unavailable', detail: 'No matching model' },
          ] };
        }
        return new Response(JSON.stringify(data), { status: 200 });
      };
      const target = (id, language = 'ja', model = 'deepseek-v4-flash') => ({
        target_language: language, profile_id: id, model, thinking_enabled: false,
      });
      const initial = {
        model_defaults: { text: { profile_id: 'deepseek', model: 'deepseek-v4-flash' }, vision: { profile_id: null, model: '' } },
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
      const refreshSettings = async () => { if (failSettingsRefresh) throw new Error('Fixture settings refresh error'); };
      let draft, setDraft, asrActions, sharedCatalog;
      function Harness() {
        [draft, setDraft] = useState(initial);
        sharedCatalog = useApiProfileViews('fixture');
        asrActions = useAsrModels({ active: false, settings: draft, onModelsChanged: noop,
          draftController: { applySettings: setDraft }, apiProfiles: profiles, providerDefinitions: definitions });
        return <WindowsCapabilities credentials={{ supported: true, available: false, reason_code: 'credentials.locked' }}>
          <TranslationSettingsSection draft={draft} apiProfiles={profiles} applySettings={setDraft} />
          <VadSettings vad={draft.vad} backend={draft.asr.backend} disabled={false} onUpdate={asrActions.updateVad} />
          <ApiManagementSettingsSection settings={draft} onRefreshSettings={refreshSettings} />
          <ExternalApiSettingsCard config={draft.external_api} saveState="saved"
            onChange={patch => setDraft(current => ({ ...current, external_api: { ...current.external_api, ...patch } }))} />
          <OcrSettingsSection vrAvailable={true} draft={draft} profiles={profiles} providerDefinitions={definitions} applySettings={setDraft} />
        </WindowsCapabilities>;
      }
      const root = createRoot(document.getElementById('root'));
      root.render(<Harness />);
      const check = (condition, message) => { if (!condition) throw new Error(message); };
      const settle = () => new Promise(resolve => setTimeout(resolve, 20));
      const fill = async (input, value) => {
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, value);
        input.dispatchEvent(new Event('input', { bubbles: true }));
        await settle();
      };
      const wait = async predicate => {
        for (let attempt = 0; attempt < 200; attempt++) {
          if (predicate()) { await settle(); return; }
          await settle();
        }
        throw new Error('Timed out: ' + predicate.toString());
      };
      const update = async patch => { setDraft(current => ({ ...current, ...patch })); await settle(); };
      const enhancement = () => document.querySelector('.translation-enhancement-row');
      const liveRouteHint = () => document.querySelector('.translation-live-route-hint');
      const addRoute = () => document.querySelector('.translation-route-add').click();
      const setRoutes = async (microphone_targets, speaker_targets = [target('deepl')], mode = 'manual', backend = 'qwen_realtime') => {
        await update({ asr: { ...draft.asr, backend }, translation: { ...draft.translation, mode, microphone_targets, speaker_targets } });
      };
      try {
        await wait(() => document.querySelectorAll('.api-profile-row').length === 6);
        check(!document.querySelector('.translation-section .model-selection-field, #settings-panel-vision .model-selection-field'),
          'Translation, OCR, and visual question pages keep model selection in model assignments');
        check(profileReads === 1 && providerReads === 1, 'API management and settings share one catalog request');
        const apiRow = document.querySelector('.api-profile-row[aria-label=deepseek]');
        const refreshButton = apiRow.querySelector('[aria-label="settings.apiManagement.refreshModels"]');
        check(refreshButton?.querySelector('svg') && !refreshButton.textContent.trim()
          && refreshButton.getAttribute('data-tooltip') === refreshButton.getAttribute('aria-label'),
          'Refresh models uses an icon with an accessible name and hover hint');
        check(!apiRow.textContent.includes('Linked:'), 'Linked features do not expand the main API list');
        check(!document.querySelector('.api-profile-row .api-profile-test-options')
          && !document.querySelector('.api-profile-row input[list]'), 'Test options belong in the editor');
        apiRow.querySelector('[aria-label="common.delete"]').click();
        await wait(() => document.querySelector('.confirm-dialog'));
        check(deletionPreviews === 1, 'Delete confirmation uses the current server preview');
        check(document.querySelector('.confirm-dialog').textContent.includes('conversation_titles'), 'Delete confirmation includes linked title generation');
        [...document.querySelectorAll('.confirm-dialog button')].find(button => button.textContent.includes('common.cancel')).click();
        await settle();
        apiRow.querySelector('[aria-label="common.edit"]').click();
        await wait(() => document.querySelector('.api-profile-editor'));
        check(!document.querySelector('.api-profile-editor [data-profile-field=api_key] input').disabled, 'A locked supported keyring still accepts an explicit replacement key');
        check(!document.querySelector('.api-profile-editor button[type=submit]').disabled, 'A locked supported keyring does not prevent an explicit save');
        check(document.querySelectorAll('.api-profile-editor [role=status]').length === 1, 'A locked credential shows one availability notice');
        check(!document.querySelector('[aria-label="settings.apiManagement.testCapability"]'), 'Each capability has its own test action without a selector');
        for (const [capability, label] of [['text_generation', 'testText'], ['text_translation', 'testTranslation']]) {
          const toggle = document.querySelector('.api-profile-capabilities [role=switch][aria-label="settings.apiManagement.capabilities.' + capability + '"]');
          check(toggle.parentElement.querySelector('[aria-label="settings.apiManagement.' + label + '"]'),
            'The test action sits beside its matching capability switch');
        }
        document.querySelector('.api-profile-editor').requestSubmit();
        await wait(() => document.querySelector('.confirm-dialog'));
        check(profileWrites === 0, 'An update with side effects waits for confirmation');
        document.querySelector('.confirm-dialog .api-danger-button').click();
        await wait(() => !document.querySelector('.confirm-dialog') && document.querySelector('.api-profile-editor [role=alert]'));
        check(document.querySelector('.api-profile-editor [role=alert]').textContent.includes('Fixture save error'), 'Save failures stay visible inside the editor');
        check(document.querySelector('.api-profile-editor input[aria-invalid=true]'), 'Server validation identifies the rejected input');
        await settle();
        check(document.querySelector('.api-profile-editor input[aria-invalid=true]') === document.activeElement,
          'Server validation highlights and focuses the rejected field; active element: ' + document.activeElement.outerHTML.slice(0, 250));
        [...document.querySelectorAll('.api-profile-editor button')].find(button => button.textContent.includes('common.cancel')).click();
        await settle();
        failSave = false;
        for (const failure of ['settings', 'catalog']) {
          failSettingsRefresh = failure === 'settings';
          failProfileReads = failure === 'catalog';
          apiRow.querySelector('[aria-label="common.edit"]').click();
          await wait(() => document.querySelector('.api-profile-editor'));
          document.querySelector('.api-profile-editor').requestSubmit();
          await wait(() => document.querySelector('.confirm-dialog'));
          document.querySelector('.confirm-dialog .api-danger-button').click();
          await wait(() => !document.querySelector('.api-profile-editor')
            || document.querySelector('.api-profile-editor [role=alert]'));
          check(!document.querySelector('.api-profile-editor'), 'A saved profile closes the editor when the ' + failure + ' refresh fails');
          check(apiRow.textContent.includes('profileSaved') && apiRow.textContent.includes('refreshFailed'), 'Save success and refresh failure remain distinct');
        }
        failSettingsRefresh = false;
        failProfileReads = false;
        check(profileReads === 3, 'A settings refresh failure does not skip the profile catalog refresh');
        for (const [id, capability, label] of [['text', 'text_generation', 'testText'], ['image', 'image_understanding', 'testImage']]) {
          const row = document.querySelector('.api-profile-row[aria-label=' + id + ']');
          row.querySelector('[aria-label="common.edit"]').click();
          await wait(() => document.querySelector('.api-profile-capability-row .api-profile-test-button'));
          const testArea = document.querySelector('.api-profile-capabilities [role=switch][aria-label="settings.apiManagement.capabilities.' + capability + '"]')
            .closest('.api-profile-capability-row');
          const testButton = () => testArea.querySelector('[aria-label="settings.apiManagement.' + label + '"]');
          check(testButton().querySelector('svg') && !testButton().textContent.trim()
            && testButton().getAttribute('data-tooltip') === testButton().getAttribute('aria-label'),
            'Test actions use an icon with an accessible name and hover hint');
          check(!document.querySelector('.api-profile-capabilities input, .api-profile-capabilities datalist'),
            'Capability tests do not require a manually entered model');
          if (id === 'text') {
            const nameInput = document.querySelector('.api-profile-editor [data-profile-field=name] input');
            await fill(nameInput, 'Unsaved name');
            check(testButton().disabled && document.querySelector('.api-profile-capabilities').textContent.includes('saveBeforeTest'),
              'Unsaved edits must be saved before testing the profile');
            await fill(nameInput, id);
          }
          check(!testButton().disabled, 'Configured capabilities can be tested without entering a model');
          testButton().click();
          await wait(() => diagnostics.some(url => new URL(url).searchParams.get('capability') === capability));
          check(!new URL(diagnostics.find(url => new URL(url).searchParams.get('capability') === capability)).searchParams.has('model'),
            'Diagnostics select the model automatically');
          await wait(() => document.querySelector('.api-profile-capabilities').textContent.includes('connectionSucceeded'));
          check(!testButton().disabled, 'The editor is available after diagnostics finish');
          if (id === 'image') {
            failDiagnostic = true;
            testButton().click();
            await wait(() => document.querySelector('.api-profile-capabilities').textContent.includes('No matching model'));
            check(!document.querySelector('.api-profile-capabilities').textContent.includes('profile.invalid'),
              'Missing test models show a model error instead of an invalid profile');
            failDiagnostic = false;
          }
          check(!row.textContent.includes('testLatency') && !row.textContent.includes('modelsAvailable')
            && !row.textContent.includes('diagnostics.checks'), 'The API list omits diagnostic details and model counts');
          [...document.querySelectorAll('.api-profile-editor-actions button')].find(button => button.textContent.includes('common.cancel')).click();
          await wait(() => !document.querySelector('.api-profile-editor'));
        }
        const textRow = document.querySelector('.api-profile-row[aria-label=text]');
        textRow.querySelector('[aria-label="common.edit"]').click();
        await wait(() => document.querySelector('.api-profile-capability-row .api-profile-test-button'));
        const textTests = document.querySelector('.api-profile-capabilities');
        deferTest = true;
        textTests.querySelector('[aria-label="settings.apiManagement.testText"]').click();
        await wait(() => textTests.querySelector('[aria-label="settings.apiManagement.cancelTest"]'));
        check(!textTests.querySelector('[aria-label="settings.apiManagement.cancelTest"]').textContent.trim(),
          'The running test keeps an icon-only cancellation action');
        check(!refreshButton.disabled,
          'Testing one profile leaves other rows available');
        textTests.querySelector('[aria-label="settings.apiManagement.cancelTest"]').click();
        await wait(() => cancelledTests === 1 && textTests.textContent.includes('testCancelled'));
        deferTest = false;
        [...document.querySelectorAll('.api-profile-editor-actions button')].find(button => button.textContent.includes('common.cancel')).click();
        await wait(() => !document.querySelector('.api-profile-editor'));
        check(profileReads === 3, 'Diagnostics do not reload the profile list');
        check(enhancement(), 'A microphone LLM route exposes global enhancement even when the speaker uses DeepL');
        failCatalogs = true;
        addRoute();
        await wait(() => draft.translation.microphone_targets.length === 2);
        check(draft.translation.microphone_targets[1].profile_id === 'deepseek'
          && draft.translation.microphone_targets[1].model === 'deepseek-v4-flash', 'Added route keeps a matching provider and model without a catalog');
        await setRoutes([target('local', 'ja', 'custom-local-model')]);
        check(document.querySelector('.translation-privacy-notice').textContent.includes('contextPrivacyLocal'), 'Local routes show local privacy scope');
        addRoute();
        await wait(() => draft.translation.microphone_targets.length === 2);
        check(draft.translation.microphone_targets[0].model === 'custom-local-model'
          && draft.translation.microphone_targets[1].model === 'deepseek-v4-flash'
          && draft.translation.microphone_targets[1].use_default_model, 'New targets follow the task default and preserve existing custom targets');
        check(document.querySelector('.translation-privacy-notice').textContent.includes('contextPrivacyCloud'), 'New targets use the configured cloud default and update the privacy scope');
        await setRoutes([target('local')], [target('deepseek')]);
        check(document.querySelector('.translation-privacy-notice').textContent.includes('contextPrivacyCloud'), 'A cloud route prevents a local-only privacy claim');
        const native = target(null, 'ja', '');
        await setRoutes([native], [native], 'automatic', 'qwen_live_translate');
        check(!enhancement(), 'Native Live routes do not expose LLM enhancement');
        check(liveRouteHint()?.textContent.includes('Qwen Live Translate'),
          'Native Live translation shows its service in a note above the target routes');
        check(!document.querySelector('.translation-route-row .translation-route-fields'),
          'The Live service name is not repeated below the target language selector');
        addRoute();
        await wait(() => draft.translation.microphone_targets.length === 2);
        check(draft.translation.microphone_targets[1].model === 'deepseek-v4-flash'
          && draft.translation.microphone_targets[1].use_default_model, 'Additional Live routes inherit a configured task model without a catalog request');
        failCatalogs = false;
        check(enhancement(), 'Extra LLM routes expose enhancement alongside native Live translation');
        await setRoutes([target('deepseek')], [target('deepl')], 'manual', 'qwen_live_translate');
        check(enhancement(), 'Manual translation exposes enhancement with a Live ASR backend');
        check(!liveRouteHint(), 'The Live route note is limited to automatic translation');

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
        document.querySelector('.api-profile-row[aria-label=asr] [aria-label="common.edit"]').click();
        await wait(() => document.querySelector('.api-profile-capability-row .api-profile-test-button'));
        document.querySelector('.api-profile-capabilities [aria-label="settings.apiManagement.testAsr"]').click();
        await wait(() => diagnostics.some(url => new URL(url).searchParams.get('capability') === 'speech_to_text'));
        check(new URL(diagnostics.find(url => new URL(url).searchParams.get('capability') === 'speech_to_text')).searchParams.get('service_id') === 'qwen_realtime', 'ASR diagnostics test the selected service instead of the first service');
        await wait(() => !document.querySelector('.api-profile-capabilities button')?.disabled);
        [...document.querySelectorAll('.api-profile-editor-actions button')].find(button => button.textContent.includes('common.cancel')).click();
        await wait(() => !document.querySelector('.api-profile-editor'));

        for (const [host, loopback] of [
          ['127.0.0.2', true], ['127.255.255.255', true], ['::1', true], ['0:0:0:0:0:0:0:1', true], ['0000::0001', true],
          ['127.256.0.1', false], ['127.00.0.1', false], ['127.1', false], ['localhost', false], ['[::1]', false],
          ['::2', false], ['192.168.0.1', false], ['0.0.0.0', false], ['::ffff:127.0.0.1', false],
        ]) {
          await update({ external_api: { ...initial.external_api } });
          const input = document.querySelector('.external-api-address-grid input');
          input.focus(); input.value = host; input.blur();
          await wait(() => draft.external_api.host === host);
          const toggle = document.querySelector('[aria-label="settings.system.externalApi.requireToken"]');
          check(toggle.disabled === !loopback && draft.external_api.require_token === !loopback, 'Token policy matches the Core IP rules: ' + host);
        }

        const bindings = () => [...document.querySelectorAll('#settings-panel-vision button')]
          .filter(button => button.textContent.includes('settings.vrOcr.bindings'));
        check(bindings().length === 1 && !bindings()[0].disabled && !document.querySelector('#vision-ocr'), 'Vision-only mode exposes one shared controller binding entry');
        document.querySelector('[role=switch][aria-label="settings.vrOcr.gesture"]').click();
        await settle();
        check(!draft.ocr.hand_gesture_enabled && draft.vision.enabled, 'Frame selection can be disabled while vision stays enabled');
        await update({ features: { ...draft.features, vr_overlay: false } });
        check(bindings()[0].disabled, 'Bindings require the VR runtime feature');
        await update({ features: { ...draft.features, vr_overlay: true, ocr: true, visual_question_answering: false },
          ocr: { ...draft.ocr, master_enabled: true, enabled: true } });
        check(bindings().length === 1 && !bindings()[0].disabled, 'OCR uses the same binding entry');
        textRow.querySelector('[aria-label="common.delete"]').click();
        await wait(() => document.querySelector('.confirm-dialog'));
        document.querySelector('.confirm-dialog .api-danger-button').click();
        await wait(() => !document.querySelector('.api-profile-row[aria-label=text]') && sharedCatalog.profiles.length === 5);
        check(profileReads === 4 && providerReads === 1, 'Mutations update all catalog subscribers without reloading provider definitions');
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
