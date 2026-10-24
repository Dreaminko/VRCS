import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const { chromium } = createRequire(import.meta.url)("playwright");

for (const field of ["model", "profile_id", "thinking_enabled"]) {
  test(`vision ${field} changes release the old test before its body settles`, { timeout: 30000 }, async () => {
    const resolveDir = join(dirname(fileURLToPath(import.meta.url)), "..");
    const bundle = await build({
      stdin: {
        resolveDir, loader: "tsx", contents: `
          import React, { useState } from 'react';
          import { createRoot } from 'react-dom/client';
          import i18next from 'i18next';
          import { initReactI18next } from 'react-i18next';
          import { VisionSettingsCard } from './src/settings/sections/VisionSettingsCard';
          import { DEFAULT_VISION_SETTINGS } from './src/settings/vision-settings';
          await i18next.use(initReactI18next).init({ lng: 'en', resources: { en: { translation: {} } } });
          window.pending = [];
          window.fetch = async (url, init) => {
            if (String(url).endsWith('/models')) return new Response(JSON.stringify({ models: ['first-model', 'second-model'] }));
            const response = new Response('{}');
            const pending = { signal: init.signal, reading: false };
            // Keep the old body pending even after abort to exercise stale finally guards.
            response.json = () => {
              pending.reading = true;
              return new Promise((resolve, reject) => {
                pending.complete = answer => resolve({ answer });
                pending.fail = () => reject(new Error('Old request failed'));
              });
            };
            window.pending.push(pending);
            return response;
          };
          const profiles = ['first-profile', 'second-profile'].map(id => ({
            id, name: id, provider: 'openai', provider_display_name: 'OpenAI',
            enabled_capabilities: ['image_understanding'],
          }));
          function Harness() {
            const [config, setConfig] = useState({ ...DEFAULT_VISION_SETTINGS, enabled: true, profile_id: 'first-profile', model: 'first-model' });
            window.patchConfig = patch => setConfig(current => ({ ...current, ...patch }));
            return <VisionSettingsCard config={config} profiles={profiles} vrAvailable={true} disabled={false}
              providerDefinitions={[]} recognition={{ backend: 'qwen_local_managed', active_profile_id: null, language: 'auto',
                managed_qwen: { package_id: 'qwen3-asr-0.6b-q8_0', device: 'auto' }, service_settings: {} }}
              onChange={window.patchConfig} onSelectProfile={profile_id => window.patchConfig({ profile_id })} />;
          }
          const root = createRoot(document.getElementById('root'));
          root.render(<Harness />);
          window.unmount = () => root.unmount();
        `,
      },
      bundle: true, write: false, format: "esm", platform: "browser", jsx: "automatic",
      define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
    });
    const browser = await chromium.launch({ headless: true, ...(process.platform === "win32" ? { channel: "msedge" } : {}) });
    try {
      const page = await browser.newPage();
      const errors = [];
      page.on("pageerror", error => errors.push(error.message));
      await page.setContent('<div id="root"></div>');
      await page.addScriptTag({ type: "module", content: bundle.outputFiles[0].text });
      const visionCard = page.locator(".vision-settings-card");
      const testResult = visionCard.locator(".vision-settings-result");
      const start = page.getByRole("button", { name: "settings.vision.testButton", exact: true });
      const testing = page.getByRole("button", { name: "settings.vision.testing", exact: true });
      await start.click();
      await page.waitForFunction(() => window.pending.length === 1 && window.pending[0].reading);
      assert.equal(await testing.isDisabled(), true);

      await page.evaluate(field => window.patchConfig({
        [field]: field === "thinking_enabled" ? true : field === "model" ? "second-model" : "second-profile",
      }), field);
      await start.waitFor();
      assert.equal(await page.evaluate(() => window.pending[0].signal.aborted), true);
      assert.equal(await start.isEnabled(), true);
      await start.click();
      await page.waitForFunction(() => window.pending.length === 2 && window.pending[1].reading);

      await page.evaluate(field => field === "model"
        ? window.pending[0].complete("Old answer") : window.pending[0].fail(), field);
      // Flush promise continuations and React's scheduled render before checking old finally.
      await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 0)));
      assert.equal(await testing.isDisabled(), true, "old finally must not reset the new test");
      assert.equal(await testResult.count(), 0);
      assert.equal(await visionCard.getByRole("alert").count(), 0);
      assert.equal(await page.evaluate(() => window.pending[1].signal.aborted), false);

      await page.evaluate(() => window.pending[1].complete("New answer"));
      await testResult.filter({ hasText: "New answer" }).waitFor();
      assert.equal(await start.isEnabled(), true);
      await start.click();
      await page.waitForFunction(() => window.pending.length === 3 && window.pending[2].reading);
      await page.evaluate(() => window.unmount());
      assert.equal(await page.evaluate(() => window.pending[2].signal.aborted), true);
      await page.evaluate(() => window.pending[2].complete("Unmounted answer"));
      await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 0)));
      assert.equal(await page.locator("#root").innerHTML(), "");
      assert.deepEqual(errors, []);
    } finally {
      await browser.close();
    }
  });
}
