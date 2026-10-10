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
        let persisted = { features: { ...DEFAULT_FEATURE_SETTINGS }, audio: { microphone: { mode: 'off', device_id: null } } };
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
            <FeatureSettingsCard features={controller.draft.features} disabled={controller.saveState === 'saving'} onChange={(key, enabled) => controller.applySettings(current => ({ ...current, features: { ...current.features, [key]: enabled } }))} />
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
    const featureHeading = featureMenu.locator("summary");
    await featureHeading.waitFor();
    assert.equal(await featureMenu.getAttribute("open"), null);
    assert.equal(await page.getByRole("switch").count(), 0);
    await featureHeading.focus();
    await featureHeading.press("Enter");
    const learning = page.getByRole("switch", { name: "Dictionary and learning", exact: true });
    await learning.waitFor({ timeout: 5000 }).catch(async () => {
      assert.fail(JSON.stringify({ errors, content: await page.locator("body").innerText() }));
    });
    assert.equal(await page.getByRole("switch").count(), 8);
    await page.screenshot({ path: join(resolveDir, "../../docs/superpowers/plans/feature-switches-ui.png"), fullPage: true, animations: "disabled" });
    await learning.click();
    assert.equal(await learning.getAttribute("aria-checked"), "false");
    assert.equal(await page.getByRole("tab", { name: "Learning", exact: true }).count(), 1);
    assert.equal(await learning.isDisabled(), true);
    await page.evaluate(() => window.completeSave(true));
    await page.getByRole("alert").waitFor();
    await page.waitForFunction(() => document.querySelector('[role=switch][aria-label="Dictionary and learning"]').getAttribute('aria-checked') === 'true');
    assert.equal(await page.getByRole("tab", { name: "Learning", exact: true }).count(), 1);
    await learning.click();
    await page.evaluate(() => window.completeSave(false));
    await page.getByRole("tab", { name: "Learning", exact: true }).waitFor({ state: "detached" });
    await learning.click();
    await page.evaluate(() => window.completeSave(false));
    await page.getByRole("tab", { name: "Learning", exact: true }).waitFor();
    const toggleWithAnimation = () => featureMenu.evaluate(async (menu) => {
      const start = menu.getBoundingClientRect().height;
      menu.querySelector("summary").click();
      const target = menu.querySelector("summary").getBoundingClientRect().height
        + (menu.open ? menu.querySelector(".settings-toggle-list").getBoundingClientRect().height : 0);
      const samples = [];
      const deadline = performance.now() + 2000;
      do {
        await new Promise(requestAnimationFrame);
        samples.push(menu.getBoundingClientRect().height);
      } while (samples.at(-1) !== target && performance.now() < deadline);
      return { start, samples, end: menu.getBoundingClientRect().height, target };
    });
    const collapse = await toggleWithAnimation();
    assert.equal(collapse.end, collapse.target);
    assert.ok(collapse.samples.some(height => collapse.start > height && height > collapse.end), JSON.stringify(collapse));
    await learning.waitFor({ state: "hidden" });
    assert.equal(await page.getByRole("switch").count(), 0);
    const expand = await toggleWithAnimation();
    assert.equal(expand.end, expand.target);
    assert.ok(expand.samples.some(height => expand.start < height && height < expand.end), JSON.stringify(expand));
    assert.equal(await learning.getAttribute("aria-checked"), "true");
    assert.equal(await page.getByRole("switch").count(), 8);
    await page.emulateMedia({ reducedMotion: "reduce" });
    await featureHeading.press("Space");
    await learning.waitFor({ state: "hidden" });
    const reducedMotion = await toggleWithAnimation();
    assert.equal(reducedMotion.end, reducedMotion.target);
    assert.ok(reducedMotion.samples.every(height => height === reducedMotion.start || height === reducedMotion.end));
    assert.equal(await page.getByRole("switch").count(), 8);
    assert.deepEqual(errors, []);
  } finally {
    await browser.close();
  }
});
