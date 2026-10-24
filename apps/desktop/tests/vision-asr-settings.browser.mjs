import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const { chromium } = createRequire(import.meta.url)("playwright");

test("vision question ASR is configurable while global live translation stays selected", { timeout: 30000 }, async t => {
  const bundle = await build({
    stdin: {
      resolveDir: join(dirname(fileURLToPath(import.meta.url)), ".."), loader: "tsx", contents: `
        import React, { useState } from 'react';
        import { createRoot } from 'react-dom/client';
        import i18next from 'i18next';
        import { initReactI18next } from 'react-i18next';
        import { VisionAsrSettings } from './src/settings/sections/VisionAsrSettings';
        import { DEFAULT_VISION_SETTINGS } from './src/settings/vision-settings';
        await i18next.use(initReactI18next).init({ lng: 'en', resources: { en: { translation: {} } } });
        window.requests = [];
        window.fetch = async url => {
          window.requests.push(String(url));
          return new Response(JSON.stringify({ models: [String(url).includes('fun_asr') ? 'fun-api-model' : 'qwen-api-model'] }));
        };
        const shared = { backend: 'qwen_live_translate', active_profile_id: 'speech', language: 'ja',
          managed_qwen: { package_id: 'qwen3-asr-0.6b-q8_0', device: 'cpu' },
          service_settings: { qwen_live_translate: { model: 'global-translation-model', context: '' },
            qwen_realtime: { model: 'saved-question-model', context: '' } } };
        const profiles = [
          { id: 'speech', name: 'Speech', provider: 'alibaba_cloud', provider_display_name: 'Alibaba Cloud', enabled_capabilities: ['speech_to_text'] },
          { id: 'text', name: 'Text only', provider: 'alibaba_cloud', provider_display_name: 'Alibaba Cloud', enabled_capabilities: ['text_generation'] },
          { id: 'stream', name: 'Stream only', provider: 'stream', provider_display_name: 'Stream', enabled_capabilities: ['speech_to_text'] },
        ];
        const service = (id, adapter) => ({ id, adapter, display_name: id, capabilities: ['speech_to_text'],
          models: [id + '-default'], model_listing: true, recognition_languages: ['auto', 'en', 'ja'] });
        const definitions = [
          { id: 'alibaba_cloud', services: [service('qwen_live_translate', 'qwen_live_translate'),
            service('qwen_realtime', 'qwen_realtime'), service('fun_asr_realtime', 'fun_asr_realtime')] },
          { id: 'stream', services: [service('openai_realtime', 'openai_realtime'), service('gemini_live_translate', 'gemini_live_translate')] },
        ];
        function Harness() {
          const [vision, setVision] = useState({ ...DEFAULT_VISION_SETTINGS, enabled: true, model: 'image-model' });
          const [disabled, setDisabled] = useState(false);
          window.vision = vision;
          window.shared = shared;
          window.setDisabled = setDisabled;
          return <VisionAsrSettings config={vision.asr} shared={shared} profiles={profiles} definitions={definitions}
            disabled={disabled} onChange={asr => setVision(current => ({ ...current, asr }))} />;
        }
        createRoot(document.getElementById('root')).render(<Harness />);
      `,
    },
    bundle: true, write: false, format: "esm", platform: "browser", jsx: "automatic",
    define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
  });
  const browser = await chromium.launch({ headless: true, ...(process.platform === "win32" ? { channel: "msedge" } : {}) });
  t.after(() => browser.close());
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.setContent('<div id="root"></div>');
  await page.addScriptTag({ type: "module", content: bundle.outputFiles[0].text });

  const source = page.getByRole("button", { name: "settings.vision.asrSource", exact: true });
  const language = page.getByRole("button", { name: "settings.recognition.language", exact: true });
  const service = page.getByRole("button", { name: "settings.recognition.cloudService", exact: true });
  const model = page.getByRole("combobox", { name: "settings.recognition.model", exact: true });
  await page.getByRole("alert").filter({ hasText: "settings.vision.asrUnsupportedGlobal" }).waitFor();
  const originalShared = await page.evaluate(() => window.shared);
  await page.evaluate(() => window.setDisabled(true));
  await page.waitForFunction(() => document.querySelector('[aria-label="settings.vision.asrSource"]').disabled);
  await page.evaluate(() => window.setDisabled(false));
  await source.click();
  assert.deepEqual(await page.getByRole("option").allTextContents(), [
    "settings.vision.asrFollowGlobal", "settings.recognition.managedQwenSource", "Speech · Alibaba Cloud",
  ]);
  await page.getByRole("option", { name: "Speech · Alibaba Cloud", exact: true }).click();
  await model.waitFor();
  await page.waitForFunction(() => window.requests.length === 1);
  assert.match(await page.evaluate(() => window.requests[0]), /\/speech\/services\/qwen_realtime\/models$/);
  assert.equal(await page.getByRole("alert").count(), 0);
  assert.equal(await model.inputValue(), "saved-question-model");

  await page.getByRole("button", { name: "settings.recognition.model", exact: true }).click();
  await page.getByRole("option", { name: "qwen-api-model", exact: true }).click();
  await model.fill("  custom-question-model  ");
  await model.press("Enter");
  await page.waitForFunction(() => window.vision.asr.model === 'custom-question-model');
  await language.click();
  await page.getByRole("option", { name: "languages.english", exact: true }).click();
  await service.click();
  assert.deepEqual(await page.getByRole("option").allTextContents(), ["qwen_realtime", "fun_asr_realtime"]);
  await page.getByRole("option", { name: "qwen_realtime", exact: true }).click();
  assert.deepEqual(await page.evaluate(() => window.vision.asr), {
    backend: "qwen_realtime", profile_id: "speech", language: "en", model: "custom-question-model",
  });
  await service.click();
  await page.getByRole("option", { name: "fun_asr_realtime", exact: true }).click();
  await page.waitForFunction(() => window.requests.length === 2);
  await page.getByRole("button", { name: "settings.recognition.model", exact: true }).click();
  await page.getByRole("option", { name: "fun-api-model", exact: true }).click();
  await page.waitForFunction(() => window.vision.asr.model === 'fun-api-model');
  assert.equal(await page.getByRole("option", { name: "qwen-api-model", exact: true }).count(), 0);

  await source.click();
  await page.getByRole("option", { name: "settings.recognition.managedQwenSource", exact: true }).click();
  await page.getByText("settings.vision.asrManagedHint", { exact: true }).waitFor();
  assert.equal(await model.count(), 0);
  assert.deepEqual(await page.evaluate(() => window.vision.asr), {
    backend: "qwen_local_managed", profile_id: null, language: "en", model: "",
  });
  await source.click();
  await page.getByRole("option", { name: "settings.vision.asrFollowGlobal", exact: true }).click();
  await page.getByRole("alert").waitFor();
  assert.equal(await page.evaluate(() => window.vision.asr), null);
  assert.equal(await page.evaluate(() => window.vision.model), "image-model");
  assert.deepEqual(await page.evaluate(() => window.shared), originalShared);
  assert.deepEqual(errors, []);
});
