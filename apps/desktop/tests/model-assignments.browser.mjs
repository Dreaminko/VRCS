import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const testDirectory = dirname(fileURLToPath(import.meta.url));
const browser = [process.env.PROGRAMFILES, process.env["PROGRAMFILES(X86)"], process.env.LOCALAPPDATA]
  .filter(Boolean).flatMap(root => [join(root, "Google/Chrome/Application/chrome.exe"), join(root, "Microsoft/Edge/Application/msedge.exe")])
  .find(existsSync);

for (const width of [1100, 380]) {
  test(`model assignments share tasks and defaults at ${width}px`, {
    timeout: 45000, skip: !browser && "No installed browser was found.",
  }, async () => {
    const bundle = await build({
      stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
        import React, { useState } from 'react';
        import { createRoot } from 'react-dom/client';
        import i18next from 'i18next';
        import { initReactI18next } from 'react-i18next';
        import en from './src/i18n/locales/en-US.json';
        import { ApiSettingsSection } from './src/settings/sections/ApiSettingsSection';
        import { applyModelDefaults } from './src/settings/model-selection';
        import { ControlTooltip } from './src/shared/ui/ControlTooltip';
        import './src/styles.css';
        document.documentElement.dataset.reducedMotion = 'true';
        await i18next.use(initReactI18next).init({ lng: 'en-US', resources: { 'en-US': en } });
        const binding = (model, use_default_model = true) => ({ profile_id: 'shared', model, use_default_model });
        const profiles = [{ id: 'shared', name: 'Shared API', provider: 'openai_compatible', provider_display_name: 'Custom API',
          enabled_capabilities: ['text_generation','text_translation','image_understanding'], credential: { configured: true },
          capabilities: { supports_model_listing: true, supports_text_generation: true, supported_languages: [] } },
        ];
        const catalogResponse = { models: ['translation-one', 'text-one', 'text-two', 'image-one'], model_details: {
          'text-one': { input_modalities: ['text'], output_modalities: ['text'] },
          'text-two': { input_modalities: ['text'], output_modalities: ['text'] },
          'translation-one': { input_modalities: ['text'], output_modalities: ['text'] },
          'image-one': { input_modalities: ['text','image'], output_modalities: ['text'] },
        } };
        let holdCatalog = false;
        let catalogHeld = false;
        let releaseCatalog;
        let catalogRequestCount = 0;
        window.fetch = async () => {
          catalogRequestCount++;
          if (holdCatalog) {
            catalogHeld = true;
            await new Promise(resolve => { releaseCatalog = resolve; });
          }
          return Response.json(catalogResponse);
        };
        let draft;
        let emptyTranslationSaves = 0;
        function Harness() {
          const [value, setValue] = useState(() => applyModelDefaults({
            model_defaults: { text: binding('text-one'), vision: binding('image-one'), translation: {
              backend: 'plamo_local', profile_id: null, model: 'plamo-3-translate-0.8b-q4_k_m',
            } },
            asr: { backend: 'qwen_local_managed' }, language_presets: [],
            translation: { mode: 'manual', model_selection: {
              backend: 'plamo_local', profile_id: null, model: 'plamo-3-translate-0.8b-q4_k_m', use_default_model: true,
            },
              speaker_targets: [{ ...binding('custom-translation', false), target_language: 'ja', thinking_enabled: false }],
              microphone_targets: [{ ...binding('text-one'), target_language: 'en', thinking_enabled: false }] },
            ocr: { model_selection: binding('text-one'), targets: [
              { ...binding('text-one'), target_language: 'zh-Hans', thinking_enabled: false },
            ] },
            conversation_titles: binding('text-one'), vision: { ...binding('image-one'), thinking_enabled: false },
            learning_ai: { ...binding('text-one'), thinking_enabled: false },
          }));
          draft = value;
          return <><ApiSettingsSection settings={value} profiles={profiles} activeTab="models"
            onRefreshSettings={async () => {}}
            applySettings={change => setValue(current => {
              const next = applyModelDefaults(change(current));
              if (next.translation.model_selection.backend === 'api'
                && !next.translation.model_selection.model.trim()) {
                emptyTranslationSaves++;
                return current;
              }
              return next;
            })} /><ControlTooltip /></>;
        }
        const root = createRoot(document.getElementById('root'));
        root.render(<Harness />);
        const check = (condition, message) => { if (!condition) throw new Error(message); };
        const settle = () => new Promise(resolve => setTimeout(resolve, 30));
        const wait = async predicate => { for (let i = 0; i < 100; i++) { if (predicate()) return; await settle(); } throw new Error('Timed out: ' + predicate); };
        const fill = async (input, value) => {
          input.focus();
          Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, value);
          input.dispatchEvent(new Event('input', { bubbles: true })); await settle(); input.blur(); await settle();
        };
        try {
          await wait(() => document.querySelectorAll('.model-default-card').length === 3);
          check([...document.querySelectorAll('.model-default-card')].every(card =>
            !card.querySelector('.model-default-actions')), 'Default model cards apply changes without save buttons');
          const translationCard = () => document.querySelectorAll('.model-default-card')[1];
          const translationToggle = translationCard().querySelector('.translation-backend-toggle.icon-only');
          check(translationToggle?.parentElement.querySelector('h3')
            && [...translationToggle.querySelectorAll('button')].every(button =>
              !button.textContent.trim() && button.querySelector('svg')),
            'Translation backend uses an icon-only selector beside the title');
          const headingRect = translationCard().querySelector('.model-default-heading').getBoundingClientRect();
          const toggleRect = translationToggle.getBoundingClientRect();
          check(headingRect.right - toggleRect.right < 1, 'Translation backend selector aligns to the right');
          const translationBackend = label => [...translationToggle.querySelectorAll('button')]
            .find(button => button.getAttribute('aria-label') === label);
          check(translationBackend('Local PLaMo').getAttribute('aria-pressed') === 'true',
            'A saved local translation default renders as local after reopening');
          check(!translationCard().querySelector('.model-selection-inherited-card')
            ?.textContent.includes('PLaMo runs on this PC'), 'Local model copy omits the API and download hint');
          await document.fonts.ready;
          await wait(() => !document.querySelector('.model-default-card .spin'));
          await settle();
          const translationEditor = translationCard().querySelector('.model-binding-editor');
          const taskGroupTop = document.querySelector('.model-assignments-group').getBoundingClientRect().top;
          let sawCatalogGap = false;
          const catalogObserver = new MutationObserver(() => {
            if (translationCard().textContent.includes('No models were returned')) sawCatalogGap = true;
          });
          catalogObserver.observe(translationEditor, { childList: true, subtree: true, characterData: true });
          holdCatalog = true;
          document.documentElement.dataset.reducedMotion = 'false';
          translationBackend('API / LLM').click();
          await settle();
          check(translationBackend('API / LLM').querySelector('svg').getAnimations().length > 0
            && translationCard().querySelector('.model-binding-editor').getAnimations().length > 0,
            'Backend selection and content switch use the shared motion');
          check(translationCard().querySelector('.model-binding-editor') === translationEditor,
            'Backend switching preserves the model editor DOM');
          document.documentElement.dataset.reducedMotion = 'true';
          check(emptyTranslationSaves === 0, 'Switching to API does not save an empty translation model');
          await wait(() => catalogHeld);
          await settle();
          const heldTaskGroupTop = document.querySelector('.model-assignments-group').getBoundingClientRect().top;
          holdCatalog = false;
          releaseCatalog();
          catalogObserver.disconnect();
          check(!sawCatalogGap, 'A pending catalog request does not render the empty-model message');
          if (innerWidth > 700) {
            check(Math.abs(heldTaskGroupTop - taskGroupTop) < 1,
              'A pending catalog request keeps the task list stable: ' + taskGroupTop + ' -> ' + heldTaskGroupTop);
          }
          await wait(() => translationBackend('API / LLM').getAttribute('aria-pressed') === 'true'
            && translationCard().querySelector('.translation-model-input-row input'));
          await wait(() => draft.model_defaults.translation.backend === 'api'
            && draft.translation.model_selection.backend === 'api'
            && draft.translation.model_selection.model === 'translation-one');
          check(draft.model_defaults.translation.profile_id === 'shared'
            && draft.model_defaults.translation.model === 'translation-one',
            'Switching to API automatically applies the available cloud model');
          const defaultDropdowns = [...document.querySelectorAll('.model-default-card .dropdown-trigger')];
          const dropdownTops = defaultDropdowns.map(dropdown => dropdown.getBoundingClientRect().top);
          if (innerWidth > 700) {
            check(defaultDropdowns.length === 3 && Math.max(...dropdownTops) - Math.min(...dropdownTops) < 1,
              'Default model dropdowns share one baseline: ' + dropdownTops.join(', '));
            const nextTaskGroupTop = document.querySelector('.model-assignments-group').getBoundingClientRect().top;
            check(Math.abs(nextTaskGroupTop - taskGroupTop) < 1,
              'Backend switching keeps the task list stable: ' + taskGroupTop + ' -> ' + nextTaskGroupTop);
          }
          check([...translationToggle.querySelectorAll('button')].every(button =>
            button.getBoundingClientRect().width <= 30), 'Translation backend selector is compact');
          const requestsBeforeLocal = catalogRequestCount;
          let refreshSpunAfterLocal = false;
          const refreshObserver = new MutationObserver(() => {
            if (translationCard().querySelector('.translation-model-input-row .spin')) {
              refreshSpunAfterLocal = true;
            }
          });
          refreshObserver.observe(translationEditor, { childList: true, subtree: true, attributes: true, attributeFilter: ['class'] });
          translationBackend('Local PLaMo').click();
          await wait(() => draft.model_defaults.translation.backend === 'plamo_local'
            && draft.translation.model_selection.backend === 'plamo_local');
          translationBackend('API / LLM').click();
          await wait(() => draft.model_defaults.translation.backend === 'api'
            && draft.translation.model_selection.backend === 'api');
          await settle();
          refreshObserver.disconnect();
          check(draft.model_defaults.translation.model === 'translation-one',
            'Switching back to API within the page restores the selected API model');
          check(catalogRequestCount === requestsBeforeLocal,
            'Switching back to API reuses the retained catalog without another request');
          check(!refreshSpunAfterLocal,
            'Switching back to API does not briefly show the catalog refresh spinner');
          const asr = draft.asr;
          const textInput = () => document.querySelector('.model-default-card input');
          await fill(textInput(), 'text-two');
          check(draft.learning_ai.model === 'text-two' && draft.conversation_titles.model === 'text-two', 'Followers update together');
          check(draft.model_defaults.translation.model === 'translation-one', 'Translation keeps its independent default');
          check(draft.translation.speaker_targets.every(target =>
            target.model === 'translation-one' && target.use_default_model),
            'Every translation target uses the shared task model');
          check(draft.asr === asr, 'Global speech recognition settings stay untouched');
          const mainList = () => document.querySelector('.model-assignment-list').textContent;
          check(!mainList().includes('Japanese') && !mainList().includes('OCR translation targets'),
            'Target-language overrides stay collapsed in task dialogs');
          check(document.querySelectorAll('.model-assignment-item').length === 5
            && !mainList().includes('Question speech recognition'),
            'Visual questions use the global speech recognition selection');
          document.querySelector('[aria-label="Edit · Subtitle translation"]').click();
          await wait(() => document.querySelector('[role=dialog]'));
          const translationDialog = document.querySelector('[role=dialog]');
          check(!translationDialog.textContent.includes('Target-language models')
            && !translationDialog.querySelector('.model-target-route, .model-selection-dialog-routes'),
            'Subtitle translation only configures its shared task model');
          document.documentElement.dataset.reducedMotion = 'false';
          const sourceButtons = [...translationDialog.querySelectorAll('.model-source-toggle button')];
          sourceButtons.find(button => button.textContent === 'Independent selection').click();
          await wait(() => translationDialog.querySelector('.model-binding-editor .translation-backend-toggle'));
          const independentEditor = translationDialog.querySelector('.model-binding-editor');
          check(independentEditor.getAnimations().length > 0,
            'Switching to an independent model reveals its editor with motion');
          const backendField = [...translationDialog.querySelectorAll('.model-binding-editor > .field')]
            .find(field => field.querySelector(':scope > span')?.textContent === 'Translation backend');
          const backendLabelRect = backendField.querySelector(':scope > span').getBoundingClientRect();
          const backendToggleRect = backendField.querySelector('.translation-backend-toggle').getBoundingClientRect();
          check(backendToggleRect.left > backendLabelRect.right
            && Math.abs((backendLabelRect.top + backendLabelRect.height / 2)
              - (backendToggleRect.top + backendToggleRect.height / 2)) < 2,
            'Independent translation backend selector stays beside its label');
          sourceButtons.find(button => button.textContent === 'Follow default').click();
          await wait(() => translationDialog.querySelector('.model-selection-inherited-card'));
          check(translationDialog.querySelector('.model-selection-inherited-card').getAnimations().length > 0,
            'Switching back to the default model reveals the inherited model with motion');
          document.querySelector('[role=dialog] .api-profile-dialog-close').click();
          await wait(() => !document.querySelector('[role=dialog]'));
          document.querySelector('[aria-label="Edit · OCR translation"]').click();
          await wait(() => document.querySelector('[role=dialog]'));
          const ocrDialog = document.querySelector('[role=dialog]');
          check(!ocrDialog.textContent.includes('OCR translation targets')
            && !ocrDialog.textContent.includes('Target-language models')
            && !ocrDialog.querySelector('.model-target-route, .model-selection-dialog-routes'),
            'OCR translation only configures its shared task model');
          document.querySelector('[role=dialog] .api-profile-dialog-close').click();
          await wait(() => !document.querySelector('[role=dialog]'));
          const editLearning = () => document.querySelector('[aria-label="Edit · Learning AI"]');
          editLearning().click(); await wait(() => document.querySelector('[role=dialog]'));
          check(document.querySelector('[role=dialog] .model-selection-dialog-heading')?.textContent === 'Learning AI', 'The model dialog renders a visible task heading');
          const sourceRow = document.querySelector('[role=dialog] .model-selection-dialog-body > .model-source-row');
          check(sourceRow?.querySelector('h3')?.textContent === 'Model source', 'The model source label renders as a heading');
          const sourceToggle = sourceRow?.querySelector('[role=group][aria-label="Model source"]');
          check(sourceToggle?.querySelectorAll('button').length === 2, 'Task model source uses a segmented control');
          if (innerWidth > 500) {
            check(sourceToggle.getBoundingClientRect().left > sourceRow.querySelector('h3').getBoundingClientRect().right,
              'The model source selector sits to the right of its heading');
          }
          [...sourceToggle.querySelectorAll('button')].find(button => button.textContent === 'Independent selection').click();
          await wait(() => document.querySelector('[role=dialog] input'));
          await fill(document.querySelector('[role=dialog] input'), 'custom-learning');
          document.querySelector('[role=dialog] .primary-button').click(); await settle();
          check(draft.learning_ai.model === 'custom-learning' && !draft.learning_ai.use_default_model, 'Tasks can override the default in the shared dialog');
          await fill(textInput(), 'text-one');
          check(draft.learning_ai.model === 'custom-learning' && draft.conversation_titles.model === 'text-one', 'Later default changes preserve task overrides');
          editLearning().dispatchEvent(new PointerEvent('pointerover', { bubbles: true, pointerType: 'mouse' }));
          await wait(() => document.querySelector('.control-tooltip'));
          check(document.querySelector('.control-tooltip').textContent === 'Change model', 'Hover uses the shared custom tooltip');
          check(!document.querySelector('.api-settings-view select, .api-settings-view [title]'), 'Model controls use custom dropdowns and hints');
          check(document.documentElement.scrollWidth <= innerWidth + 1, 'The settings view fits the viewport');
          document.getElementById('result').textContent = 'PASS';
        } catch (error) { document.getElementById('result').textContent = 'FAIL: ' + error.message; }
        parent.postMessage(document.getElementById('result').textContent, '*');
      ` },
      bundle: true, write: false, outfile: "model-assignments.js", format: "esm", platform: "browser", jsx: "automatic",
      define: { "import.meta.env": "{}" },
    });
    const directory = mkdtempSync(join(testDirectory, ".model-assignments-browser-"));
    try {
      const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
      const stylesheet = bundle.outputFiles.find(file => file.path.endsWith(".css")).text;
      writeFileSync(join(directory, "content.html"), '<style>' + stylesheet + '</style><div id="root" class="settings-surface" style="padding:24px"></div>'
        + '<pre id="result">PENDING</pre><script type="module">' + javascript.replace(/<\/script/gi, "<\\/script") + '</script>');
      const html = join(directory, "test.html");
      writeFileSync(html, '<body style="margin:0"><iframe src="content.html" style="border:0;width:' + width
        + 'px;height:1200px"></iframe><pre id="result">PENDING</pre><script>window.addEventListener("message",event=>document.getElementById("result").textContent=event.data);</script></body>');
      const screenshot = process.env.VRCS_MODEL_SCREENSHOT_DIR;
      const result = spawnSync(browser, ["--headless=new", "--no-sandbox", "--dump-dom", "--virtual-time-budget=12000", "--disable-gpu",
        "--no-first-run", "--disable-background-networking", "--disable-extensions", "--window-size=" + Math.max(500, width) + ",1200",
        ...(screenshot ? ["--screenshot=" + join(screenshot, `models-${width}.png`)] : []),
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
