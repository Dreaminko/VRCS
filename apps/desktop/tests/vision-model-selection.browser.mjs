// Run with Playwright available to Node: node --test tests/vision-model-selection.browser.mjs
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const { chromium } = createRequire(import.meta.url)("playwright");

test("vision model selection uses provider catalogs and preserves saved models on failures", { timeout: 30000 }, async (t) => {
  const bundle = await build({
    stdin: {
      resolveDir: join(dirname(fileURLToPath(import.meta.url)), ".."),
      loader: "tsx",
      contents: `
        import React, { useState } from 'react';
        import { createRoot } from 'react-dom/client';
        import i18next from 'i18next';
        import { initReactI18next } from 'react-i18next';
        import { VisionSettingsCard } from './src/settings/sections/VisionSettingsCard';
        import { DEFAULT_VISION_SETTINGS, selectVisionProfile } from './src/settings/vision-settings';
        await i18next.use(initReactI18next).init({ lng: 'en', resources: { en: { translation: {} } } });
        window.catalogs = { first: ['first-model', 'alternate-model', 'text-only'], second: ['second-model'] };
        window.details = { first: {
          'first-model': { input_modalities: ['text', 'image'], output_modalities: ['text'], context_tokens: 4096,
            reasoning: { supported: true, mandatory: false, supported_efforts: ['none', 'medium'] } },
          'text-only': { input_modalities: ['text'], output_modalities: ['text'] },
        } };
        window.requests = [];
        window.failCatalog = false;
        window.delaySecond = false;
        window.delayFirst = false;
        window.fetch = async url => {
          const id = String(url).split('/profiles/')[1].split('/')[0];
          window.requests.push(id);
          const catalog = { models: window.catalogs[id], model_details: window.details[id] };
          if (window.failCatalog) throw new Error('Catalog unavailable');
          if (id === 'second' && window.delaySecond) {
            await new Promise(resolve => { window.finishSecond = resolve; });
          }
          if (id === 'first' && window.delayFirst) {
            await new Promise(resolve => { window.finishFirst = resolve; });
          }
          return new Response(JSON.stringify(catalog));
        };
        const profiles = ['first', 'second'].map(id => ({
          id, name: id, provider: 'openai', provider_display_name: 'OpenAI',
          enabled_capabilities: ['image_understanding'],
        }));
        function Harness() {
          const [currentProfiles, setProfiles] = useState(profiles);
          window.setRegion = region => setProfiles(current => current.map(profile => ({ ...profile, region })));
          const [config, setConfig] = useState({ ...DEFAULT_VISION_SETTINGS, profile_id: 'first', model: 'first-model' });
          const [disabled, setDisabled] = useState(false);
          window.setDisabled = setDisabled;
          window.config = config;
          return <VisionSettingsCard config={config} profiles={currentProfiles} vrAvailable={true} disabled={disabled}
            onChange={patch => setConfig(current => ({ ...current, ...patch }))}
            onSelectProfile={id => setConfig(current => selectVisionProfile({ vision: current }, id).vision)} />;
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
  const model = page.getByRole("combobox", { name: "settings.vision.model", exact: true });
  const modelToggle = page.locator(".translation-model-input-row .editable-dropdown-toggle");
  const profile = page.getByRole("button", { name: "settings.vision.profile", exact: true });
  const refresh = page.getByRole("button", { name: "settings.apiManagement.refreshModels", exact: true });
  await page.waitForFunction(() => window.requests.length === 1);
  await page.getByRole("switch", { name: "settings.vision.enable", exact: true }).click();
  await page.waitForFunction(() => !document.querySelector('.translation-model-input-row .editable-dropdown-toggle').disabled);
  assert.equal(await page.locator('input[role="combobox"]').count(), 1);

  const thinking = page.getByRole("switch", { name: "settings.translation.thinkingMode", exact: true });
  await thinking.waitFor();
  assert.equal(await thinking.getAttribute("aria-checked"), "false");
  await thinking.click();
  await page.waitForFunction(() => window.config.thinking_enabled === true);
  await page.evaluate(() => window.setDisabled(true));
  await page.waitForFunction(() => document.querySelector('[aria-label="settings.translation.thinkingMode"]').disabled);
  await page.evaluate(() => window.setDisabled(false));
  await page.waitForFunction(() => !document.querySelector('[aria-label="settings.translation.thinkingMode"]').disabled);

  await modelToggle.click();
  assert.equal(await page.getByRole("option", { name: "text-only", exact: true }).count(), 0);
  await model.fill("alternate");
  await page.getByRole("option", { name: "alternate-model", exact: true }).click();
  await page.waitForFunction(() => window.config.model === "alternate-model");
  assert.equal(await thinking.count(), 0);
  assert.equal(await page.evaluate(() => window.config.thinking_enabled), true);
  assert.equal(await model.inputValue(), "alternate-model");

  await page.evaluate(() => { window.failCatalog = true; });
  await refresh.click();
  await page.getByRole("alert").waitFor();
  assert.equal(await page.evaluate(() => window.config.model), "alternate-model");
  assert.equal(await model.isDisabled(), false);
  assert.equal(await modelToggle.isDisabled(), true);
  assert.equal(await model.inputValue(), "alternate-model");

  await page.evaluate(() => { window.failCatalog = false; window.catalogs.first = []; });
  await refresh.click();
  await page.getByText("settings.vision.noModels", { exact: true }).waitFor();
  assert.equal(await page.evaluate(() => window.config.model), "alternate-model");
  assert.equal(await model.isDisabled(), false);
  assert.equal(await modelToggle.isDisabled(), true);

  await page.evaluate(() => { window.catalogs.first = ['first-model']; });
  await refresh.click();
  await page.getByText("settings.vision.savedModelUnavailable", { exact: true }).waitFor();
  await page.waitForFunction(() => !document.querySelector('.translation-model-input-row .editable-dropdown-toggle').disabled);
  assert.equal(await page.evaluate(() => window.config.model), "alternate-model");

  await page.evaluate(() => { window.delaySecond = true; });
  await profile.click();
  await page.getByRole("option", { name: "second · OpenAI", exact: true }).click();
  await page.waitForFunction(() => Boolean(window.finishSecond));
  assert.equal(await page.evaluate(() => window.config.model), "");
  assert.equal(await modelToggle.isDisabled(), true);
  await profile.click();
  await page.getByRole("option", { name: "first · OpenAI", exact: true }).click();
  await page.waitForFunction(() => !document.querySelector('.translation-model-input-row .editable-dropdown-toggle').disabled);
  await page.evaluate(() => window.finishSecond());
  await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 0)));
  await modelToggle.click();
  assert.equal(await page.getByRole("option", { name: "second-model", exact: true }).count(), 0);
  await page.getByRole("option", { name: "first-model", exact: true }).click();
  await page.waitForFunction(() => window.config.model === "first-model");
  await thinking.waitFor();
  assert.equal(await thinking.getAttribute("aria-checked"), "true");
  await thinking.click();
  await page.waitForFunction(() => window.config.thinking_enabled === false);

  await modelToggle.click();
  await page.getByRole("option", { name: "settings.vision.selectModel", exact: true }).click();
  await page.waitForFunction(() => window.config.model === "");
  assert.equal(await page.getByRole("switch", { name: "settings.vision.enable", exact: true }).isDisabled(), false);
  assert.equal(await page.evaluate(() => window.config.enabled), true);

  // A region change on the same profile must discard an older pending catalog.
  await page.evaluate(() => {
    window.delayFirst = true;
    window.catalogs.first = ['first-model', 'unknown'];
    window.setRegion('singapore');
  });
  await page.waitForFunction(() => Boolean(window.finishFirst));
  await page.evaluate(() => {
    window.delayFirst = false;
    window.details.first = { 'first-model': { input_modalities: ['text'], output_modalities: ['text'] } };
    window.setRegion('china_beijing');
  });
  await page.waitForFunction(() => !document.querySelector('.translation-model-input-row .editable-dropdown-toggle').disabled);
  await page.evaluate(() => window.finishFirst());
  await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 0)));
  await modelToggle.click();
  assert.equal(await page.getByRole("option", { name: "first-model", exact: true }).count(), 0);
  await model.fill("  custom-vision-model  ");
  await model.press("Enter");
  await page.waitForFunction(() => window.config.model === "custom-vision-model");
  assert.deepEqual(errors, []);
});
