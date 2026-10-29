import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const { chromium } = createRequire(import.meta.url)("playwright");

test("feature switches commit menu visibility and restore the switch on save failure", async () => {
  const resolveDir = join(dirname(fileURLToPath(import.meta.url)), "..");
  const bundle = await build({
    stdin: {
      resolveDir, loader: "tsx", contents: `
        import React from 'react';
        import { createRoot } from 'react-dom/client';
        import i18next from 'i18next';
        import { initReactI18next } from 'react-i18next';
        import en from './src/i18n/locales/en-US.json';
        import { useSettingsRuntime } from './src/settings/hooks/useSettingsRuntime';
        import { useSettingsDraft } from './src/settings/hooks/useSettingsDraft';
        import { FeatureSettingsCard } from './src/settings/system/FeatureSettingsCard';
        import { SettingsTabBar } from './src/settings/components/SettingsTabBar';
        import { DEFAULT_FEATURE_SETTINGS, visibleSettingsCategories } from './src/settings/feature-availability';
        await i18next.use(initReactI18next).init({ lng: 'en-US', resources: { 'en-US': { translation: en.translation } } });
        let persisted = { features: { ...DEFAULT_FEATURE_SETTINGS }, audio: { output: { mode: 'disabled' }, microphone: { mode: 'disabled', device_id: null } } };
        window.savedFeatures = () => persisted.features;
        window.fetch = async (_url, init) => {
          if (init?.method === 'PUT') {
            return await new Promise(resolve => {
              window.completeSave = (fail) => {
                if (!fail) persisted = JSON.parse(init.body);
                resolve(new Response(JSON.stringify(fail ? { code: 'settings.invalid', detail: 'Rejected for test', params: {} } : persisted), { status: fail ? 422 : 200 }));
              };
            });
          }
          return new Response(JSON.stringify(persisted), { status: 200 });
        };
        const noop = () => {};
        const health = { getCurrent: () => null, refreshQuietly: async () => {} };
        function Features({ settings, save }) {
          const controller = useSettingsDraft(settings, save);
          return <div className="settings-section settings-section-active system-section">
            <SettingsTabBar activeCategory="system" visibleCategories={visibleSettingsCategories(settings.features)} onChange={noop} />
            <FeatureSettingsCard features={controller.draft.features} disabled={false} onChange={(key, enabled) => controller.applySettings(current => ({ ...current, features: { ...current.features, [key]: enabled } }))} />
            {controller.saveMessage && <p role="alert">{controller.saveMessage}</p>}
          </div>;
        }
        function Harness() {
          const runtime = useSettingsRuntime({ active: false, coreConfigured: true, health, stopMicrophoneTest: async () => {}, clearErrorFrom: noop, reportError: noop });
          return runtime.value ? <Features settings={runtime.value} save={runtime.save} /> : null;
        }
        createRoot(document.getElementById('root')).render(<Harness />);
      `,
    },
    bundle: true, write: false, format: "esm", platform: "browser", jsx: "automatic",
    define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
  });
  const css = await build({ entryPoints: [join(resolveDir, "src/styles.css")], bundle: true, write: false });
  const browser = await chromium.launch({ headless: true, ...(process.platform === "win32" ? { channel: "msedge" } : {}) });
  try {
    const page = await browser.newPage({ viewport: { width: 1200, height: 900 } });
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    await page.setContent('<div id="root" style="max-width:1080px;margin:24px auto"></div>');
    await page.addStyleTag({ content: css.outputFiles[0].text });
    await page.addScriptTag({ type: "module", content: bundle.outputFiles[0].text });
    const featureMenu = page.locator(".system-features-group");
    const learning = page.getByRole("switch", { name: "Dictionary and learning", exact: true });
    await learning.waitFor({ timeout: 5000 }).catch(async () => {
      assert.fail(JSON.stringify({ errors, content: await page.locator("body").innerText() }));
    });
    const speakerFilter = featureMenu.getByRole("switch", { name: "Microphone speaker blocking", exact: true });
    assert.equal(await featureMenu.getByRole("switch", { name: "OSC Chatbox", exact: true }).count(), 0);
    assert.equal(await speakerFilter.getAttribute("aria-checked"), "false");
    await speakerFilter.click();
    assert.equal(await speakerFilter.getAttribute("aria-checked"), "true");
    await page.evaluate(() => window.completeSave(true));
    await page.getByRole("alert").waitFor();
    await page.waitForFunction(() => document.querySelector('[role=switch][aria-label="Microphone speaker blocking"]').getAttribute('aria-checked') === 'false');
    await speakerFilter.click();
    await page.evaluate(() => window.completeSave(false));
    await page.waitForFunction(() => window.savedFeatures().microphone_speaker_filter === true);
    assert.equal(await page.evaluate(() => "osc_chatbox" in window.savedFeatures()), false);
    await learning.click();
    await page.evaluate(() => window.completeSave(false));
    await page.getByRole("tab", { name: "Learning", exact: true }).waitFor({ state: "detached" });
    await learning.click();
    await page.evaluate(() => window.completeSave(false));
    await page.getByRole("tab", { name: "Learning", exact: true }).waitFor();
    assert.deepEqual(errors, []);
  } finally {
    await browser.close();
  }
});
