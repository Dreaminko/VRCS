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
  test(`model assignments share defaults and preserve overrides at ${width}px`, {
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
          capabilities: { supports_model_listing: true, supports_text_generation: true, supported_languages: [] } }];
        window.fetch = async () => Response.json({ models: ['text-one', 'text-two', 'image-one'], model_details: {
          'text-one': { input_modalities: ['text'], output_modalities: ['text'] },
          'text-two': { input_modalities: ['text'], output_modalities: ['text'] },
          'image-one': { input_modalities: ['text','image'], output_modalities: ['text'] },
        } });
        let draft;
        function Harness() {
          const [value, setValue] = useState(() => applyModelDefaults({
            model_defaults: { text: binding('text-one'), vision: binding('image-one') },
            asr: { backend: 'qwen_local_managed' }, language_presets: [],
            translation: { mode: 'manual', model_selection: binding('text-one'),
              speaker_targets: [{ ...binding('custom-translation', false), target_language: 'ja', thinking_enabled: false }],
              microphone_targets: [{ ...binding('text-one'), target_language: 'en', thinking_enabled: false }] },
            ocr: { model_selection: binding('text-one'), targets: [] },
            conversation_titles: binding('text-one'), vision: { ...binding('image-one'), thinking_enabled: false },
            learning_ai: { ...binding('text-one'), thinking_enabled: false },
          }));
          draft = value;
          return <><ApiSettingsSection settings={value} profiles={profiles} onRefreshSettings={async () => {}}
            applySettings={change => setValue(current => applyModelDefaults(change(current)))} /><ControlTooltip /></>;
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
          await wait(() => document.querySelectorAll('.model-default-card input').length === 2);
          const asr = draft.asr;
          const textInput = () => document.querySelector('.model-default-card input');
          await fill(textInput(), 'text-two');
          document.querySelector('.model-default-card .primary-button').click(); await settle();
          check(draft.learning_ai.model === 'text-two' && draft.conversation_titles.model === 'text-two', 'Followers update together');
          check(draft.translation.speaker_targets[0].model === 'custom-translation', 'Independent translation targets survive');
          check(draft.asr === asr && !document.querySelector('.model-assignment-list').textContent.includes('Speech recognition'), 'Recognition stays in its original section');
          check(document.querySelector('.model-assignment-list').textContent.includes('Japanese')
            && !document.querySelector('.model-assignment-list').textContent.includes('OCR translation targets'),
            'Subroutes use localized language names and hide empty target groups');
          const editLearning = () => document.querySelector('[aria-label="Edit · Learning AI"]');
          editLearning().click(); await wait(() => document.querySelector('[role=dialog]'));
          check(document.querySelector('[role=dialog] .model-selection-dialog-heading')?.textContent === 'Learning AI', 'The model dialog renders a visible task heading');
          document.querySelector('[aria-label="Model source"]').click();
          await wait(() => [...document.querySelectorAll('[role=option]')].some(option => option.textContent === 'Independent selection'));
          [...document.querySelectorAll('[role=option]')].find(option => option.textContent === 'Independent selection').click();
          await wait(() => document.querySelector('[role=dialog] input'));
          await fill(document.querySelector('[role=dialog] input'), 'custom-learning');
          document.querySelector('[role=dialog] .primary-button').click(); await settle();
          check(draft.learning_ai.model === 'custom-learning' && !draft.learning_ai.use_default_model, 'Tasks can override the default in the shared dialog');
          await fill(textInput(), 'text-one');
          document.querySelector('.model-default-card .primary-button').click(); await settle();
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
