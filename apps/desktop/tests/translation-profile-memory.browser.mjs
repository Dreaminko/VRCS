// Run with Playwright available to Node: node --test tests/translation-profile-memory.browser.mjs
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const { chromium } = createRequire(import.meta.url)("playwright");

test("translation profile switching remembers edited models across remounts and catalog failures", async (t) => {
  const bundle = await build({
    stdin: {
      resolveDir: join(dirname(fileURLToPath(import.meta.url)), ".."),
      loader: "tsx",
      contents: `
        import React, { useState } from 'react';
        import { createRoot } from 'react-dom/client';
        import i18next from 'i18next';
        import { initReactI18next } from 'react-i18next';
        import { TranslationRouteList } from './src/settings/translation/TranslationRouteList.tsx';
        await i18next.use(initReactI18next).init({ lng: 'en', resources: { en: { translation: {} } } });
        const catalogs = { deepseek: ['deepseek-v4-flash', 'deepseek-v4-pro'], openai: ['gpt-4.1-mini', 'gpt-5-mini'] };
        window.failCatalogs = false;
        window.fetch = async (url) => {
          if (window.failCatalogs) throw new Error('Catalog unavailable');
          const id = String(url).includes('/deepseek/') ? 'deepseek' : 'openai';
          return new Response(JSON.stringify({ models: catalogs[id], configured: true }), { status: 200 });
        };
        const profiles = ['deepseek', 'openai'].map(id => ({
          id, name: id, provider: id, provider_display_name: id,
          enabled_capabilities: ['text_generation', 'text_translation'],
          capabilities: { supports_model_listing: true, supports_text_generation: true,
            supports_translation: true, supported_languages: ['en', 'ja'], supports_custom_translation_language: true }
        }));
        window.savedTargets = [{ target_language: 'ja', profile_id: 'deepseek', model: 'deepseek-v4-pro', thinking_enabled: false }];
        function Harness() {
          const [targets, setTargets] = useState(window.savedTargets);
          return <TranslationRouteList title="Translation" targets={targets} profiles={profiles} disabled={false}
            onChange={next => { window.savedTargets = JSON.parse(JSON.stringify(next)); setTargets(next); }} />;
        }
        let root = createRoot(document.getElementById('root'));
        root.render(<Harness />);
        window.remount = () => { root.unmount(); root = createRoot(document.getElementById('root')); root.render(<Harness />); };
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
  await page.setContent('<div id="root"></div><button id="outside">Outside</button>');
  await page.addScriptTag({ type: "module", content: bundle.outputFiles[0].text });
  const model = page.locator('input[role="combobox"]');
  async function switchTo(id, expected) {
    await page.locator(".translation-route-row .dropdown-trigger").last().click();
    await page.getByRole("option", { name: id, exact: true }).click();
    await page.waitForFunction(({ id, expected }) => window.savedTargets[0].profile_id === id && window.savedTargets[0].model === expected, { id, expected });
    assert.equal(await model.inputValue(), expected);
  }
  await model.waitFor();
  await switchTo("openai", "gpt-4.1-mini");
  await model.fill("gpt-5-mini");
  await page.locator("#outside").click();
  await page.waitForFunction(() => window.savedTargets[0].model === "gpt-5-mini");
  await switchTo("deepseek", "deepseek-v4-pro");
  await page.evaluate(() => { window.failCatalogs = true; window.remount(); });
  await model.waitFor();
  await switchTo("openai", "gpt-5-mini");
  await switchTo("deepseek", "deepseek-v4-pro");
  assert.deepEqual(errors, []);
});
