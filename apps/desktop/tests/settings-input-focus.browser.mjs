// Run with Playwright available to Node: node --test tests/settings-input-focus.browser.mjs
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test, { after, before } from "node:test";
import { build } from "esbuild";

const { chromium } = createRequire(import.meta.url)("playwright");
let browser;
let script;

before(async () => {
  const bundle = await build({
    stdin: {
      resolveDir: join(dirname(fileURLToPath(import.meta.url)), ".."),
      loader: "tsx",
      contents: `
        import React, { useState } from 'react';
        import { createRoot } from 'react-dom/client';
        import i18next from 'i18next';
        import { initReactI18next } from 'react-i18next';
        import { TranslationSettingsSection } from './src/settings/sections/TranslationSettingsSection.tsx';
        import { OcrSettingsSection } from './src/settings/sections/OcrSettingsSection.tsx';
        import { useSettingsDraft } from './src/settings/hooks/useSettingsDraft.ts';
        import { DEFAULT_OCR_SETTINGS } from './src/settings/vr-overlay-settings.ts';
        import { EditableDropdownField } from './src/shared/ui/DropdownField.tsx';
        await i18next.use(initReactI18next).init({ lng: 'en', resources: { en: { translation: {} } } });
        window.fetch = async () => new Response(JSON.stringify({ models: ['gpt-5-mini', 'gpt-5'], configured: true }), { status: 200 });
        window.savedSettings = [];
        window.modelUpdates = [];
        const profile = { id: 'p1', name: 'Test', provider_display_name: 'Test', provider: 'openai_compatible', is_local: true, enabled_capabilities: ['text_generation', 'text_translation'], capabilities: { supports_streaming: false, requires_api_key: false, is_local: true, supports_model_listing: true, supports_context: true, supports_translation: true, supports_text_generation: true, supports_asr: false, supported_languages: ['en', 'ja'], supports_custom_translation_language: false } };
        const target = { target_language: 'ja', profile_id: 'p1', model: 'gpt-5-mini', thinking_enabled: false };
        const initial = { features: { vr_overlay: true, ocr: true }, asr: { backend: 'qwen_local_managed', language: 'auto', managed_qwen: { package_id: 'qwen3-asr-0.6b-q8_0', device: 'auto' }, active_profile_id: null, service_settings: {} }, translation: { mode: 'manual', speaker_targets: [target], microphone_targets: [target], prompt: { system_prompt: 'Translate.{glossary}{context}', context_enabled: true, include_speaker: true, include_microphone: true, include_chatbox: true, max_messages: 2, max_chars: 1000 } }, language_presets: [{ id: 'preset1', name: 'Original', recognition_language: 'en', speaker_targets: [target], microphone_targets: [target], translation_mode: 'manual', osc_translation_strategy: 'preferred_only' }], ocr: { ...DEFAULT_OCR_SETTINGS, targets: [target] } };
        const onSave = async (settings) => {
          window.savedSettings.push(settings);
          await new Promise(resolve => setTimeout(resolve, 30));
          return settings;
        };
        function Harness() {
          const { draft, saveState, applySettings } = useSettingsDraft(initial, onSave);
          const [model, setModel] = useState('original');
          return <>
            <TranslationSettingsSection draft={draft} saveState={saveState} applySettings={applySettings} apiProfiles={[profile]} />
            <OcrSettingsSection vrAvailable={true} draft={draft} saveState={saveState} applySettings={applySettings} profiles={[profile]} />
            <div id="immediate-model"><EditableDropdownField label="Immediate model" value={model} options={[]} onChange={(next) => { window.modelUpdates.push(next); setModel(next); }} /></div>
          </>;
        }
        createRoot(document.getElementById('root')).render(<Harness />);
      `,
    },
    bundle: true, write: false, format: "esm", platform: "browser", jsx: "automatic",
    define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
  });
  script = bundle.outputFiles[0].text;
  browser = await chromium.launch({ headless: true, ...(process.platform === "win32" ? { channel: "msedge" } : {}) });
});

after(async () => { await browser?.close(); });

async function openPage(t) {
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  t.after(async () => { await page.close(); assert.deepEqual(errors, []); });
  await page.setContent('<div id="root"></div><button id="outside">Outside</button>');
  await page.addScriptTag({ type: "module", content: script });
  await page.locator("input[role=combobox]").first().waitFor();
  return page;
}

for (const [label, selector, suffix, savedValue] of [
  ["self translation model", "#settings-panel-translation input[role=combobox] >> nth=0", "-custom", (s) => s.translation.microphone_targets[0].model],
  ["other-party translation model", "#settings-panel-translation input[role=combobox] >> nth=1", "-custom", (s) => s.translation.speaker_targets[0].model],
  ["OCR translation model", "#settings-panel-ocr input[role=combobox]", "-custom", (s) => s.ocr.targets[0].model],
  ["context message limit", "#settings-panel-translation input[type=number] >> nth=0", "0", (s) => s.translation.prompt.max_messages],
  ["context character limit", "#settings-panel-translation input[type=number] >> nth=1", "0", (s) => s.translation.prompt.max_chars],
  ["preset name", ".translation-preset-row input", "New", (s) => s.language_presets[0].name],
]) {
  test(`${label} keeps focus while typing and saves once on blur`, async (t) => {
    const page = await openPage(t);
    const input = page.locator(selector);
    const original = await input.inputValue();
    await input.focus();
    await page.keyboard.press("End");
    await page.keyboard.type(suffix);
    assert.equal(await input.evaluate((el) => document.activeElement === el), true);
    assert.equal(await input.inputValue(), original + suffix);
    assert.deepEqual(await page.evaluate(() => window.savedSettings), []);
    await page.locator("#outside").click();
    await page.waitForFunction(() => window.savedSettings.length === 1);
    const saved = await page.evaluate(() => window.savedSettings);
    assert.equal(savedValue(saved[0]), selector.includes("type=number") ? Number(original + suffix) : original + suffix);
    await page.waitForFunction(() => !document.querySelector(".translation-preset-row input").disabled);
    await input.focus();
    await page.locator("#outside").click();
    assert.equal(await page.evaluate(() => window.savedSettings.length), 1);
  });
}

test("model selection commits the selected option without saving an unfinished draft", async (t) => {
  const page = await openPage(t);
  const field = page.locator(".editable-dropdown-field").first();
  const input = field.locator("input");
  await input.fill("unfinished");
  await field.locator(".editable-dropdown-toggle").click();
  await field.getByRole("option", { name: "gpt-5", exact: true }).click();
  const saved = await page.evaluate(() => window.savedSettings);
  assert.equal(saved.length, 1);
  assert.equal(saved[0].translation.microphone_targets[0].model, "gpt-5");
});

test("context limits allow clearing and retyping, and discard invalid values", async (t) => {
  const page = await openPage(t);
  const input = page.locator("#settings-panel-translation input[type=number]").first();
  await input.fill("");
  assert.equal(await input.inputValue(), "");
  assert.deepEqual(await page.evaluate(() => window.savedSettings), []);
  await page.keyboard.type("35");
  await page.keyboard.press("Enter");
  await page.waitForFunction(() => window.savedSettings.length === 1);
  assert.equal((await page.evaluate(() => window.savedSettings))[0].translation.prompt.max_messages, 35);
  await page.waitForFunction(() => !document.querySelector("input[type=number]").disabled);
  for (const invalid of ["", "0", "51", "1.5"]) {
    await input.fill(invalid);
    await page.locator("#outside").click();
    assert.equal(await input.inputValue(), "35");
    assert.equal(await page.evaluate(() => window.savedSettings.length), 1);
  }
});

test("context character limits preserve valid integers and reject values outside the allowed range", async (t) => {
  const page = await openPage(t);
  const input = page.locator("#settings-panel-translation input[type=number]").nth(1);
  await input.fill("250");
  await page.locator("#outside").click();
  await page.waitForFunction(() => window.savedSettings.length === 1);
  assert.equal((await page.evaluate(() => window.savedSettings))[0].translation.prompt.max_chars, 250);
  await page.waitForFunction(() => !document.querySelector("input[type=number]").disabled);
  for (const invalid of ["", "199", "12001", "250.5"]) {
    await input.fill(invalid);
    await page.locator("#outside").click();
    assert.equal(await input.inputValue(), "250");
    assert.equal(await page.evaluate(() => window.savedSettings.length), 1);
  }
});

test("Enter saves a model name but does not interrupt IME composition", async (t) => {
  const page = await openPage(t);
  const input = page.locator("#settings-panel-translation input[role=combobox]").first();
  await input.fill("custom-model");
  await input.evaluate((el) => el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, isComposing: true })));
  assert.equal(await input.evaluate((el) => document.activeElement === el), true);
  assert.deepEqual(await page.evaluate(() => window.savedSettings), []);
  await page.keyboard.press("Enter");
  await page.waitForFunction(() => window.savedSettings.length === 1);
  assert.equal((await page.evaluate(() => window.savedSettings))[0].translation.microphone_targets[0].model, "custom-model");
});

test("editable dropdowns retain immediate updates unless blur commits are requested", async (t) => {
  const page = await openPage(t);
  const input = page.locator("#immediate-model input");
  await input.focus();
  await page.keyboard.press("End");
  await page.keyboard.type("X");
  assert.equal(await input.inputValue(), "originalX");
  assert.deepEqual(await page.evaluate(() => window.modelUpdates), ["originalX"]);
  assert.equal(await input.evaluate((el) => document.activeElement === el), true);
});
