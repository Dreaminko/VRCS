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

for (const width of [1200, 500]) {
  test(`shared reasoning controls validate, retain and restore settings at ${width}px`, {
    timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
  }, async () => {
    const bundle = await build({
      stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
        import React, { useEffect, useState } from 'react';
        import { createRoot } from 'react-dom/client';
        import i18next from 'i18next';
        import { initReactI18next } from 'react-i18next';
        import './src/styles.css';
        import en from './src/i18n/locales/en-US.json';
        import { updateReducedMotionPreference } from './src/app/app-motion';
        import { learningPreferencesSnapshot, readLearningPreferences, updateLearningPreferences } from './src/learning/preferences';
        import { providersApi } from './src/providers/api';
        import { notifyApiProfilesChanged } from './src/providers/api-profile-events';
        import { ReasoningControlField } from './src/settings/ReasoningControlField';
        import { LearningAiSettingsSection } from './src/settings/sections/LearningAiSettingsSection';

        await i18next.use(initReactI18next).init({ lng: 'en-US', resources: { 'en-US': { translation: en.translation } } });
        updateReducedMotionPreference(true);
        window.requestAnimationFrame = callback => window.setTimeout(() => callback(performance.now()), 16);
        window.cancelAnimationFrame = id => window.clearTimeout(id);
        const storageKey = 'vrcs.reasoning-controls.browser-test';
        localStorage.removeItem(storageKey);
        const initial = {
          mandatory: { thinking_enabled: false, reasoning_control: null },
          optional: { thinking_enabled: true, reasoning_control: { type: 'effort', value: 'high' } },
          budget: { thinking_enabled: false, reasoning_control: null },
        };
        const fixtures = {
          mandatory: { provider: 'groq', model: 'openai/gpt-oss-120b', details: { reasoning: {
            supported: true, mandatory: true, can_disable: false, supported_efforts: ['low', 'medium', 'high'],
          } } },
          optional: { provider: 'openai', model: 'gpt-5.4', details: { reasoning: {
            supported: true, mandatory: false, can_disable: true, supported_efforts: ['none', 'low', 'medium', 'high'],
          } } },
          budget: { provider: 'gemini', model: 'gemini-2.5-pro', details: { reasoning: {
            supported: true, mandatory: true, can_disable: false, supports_dynamic_budget: true,
            controls: [{ type: 'budget_tokens', min: 128, max: 8192 }],
          } } },
        };
        let settings;
        function Harness() {
          const [state, setState] = useState(() => JSON.parse(localStorage.getItem(storageKey) ?? 'null') ?? initial);
          settings = state;
          useEffect(() => { localStorage.setItem(storageKey, JSON.stringify(state)); }, [state]);
          return <div className="learning-ai-settings-grid">
            {Object.entries(fixtures).map(([id, fixture]) => <section key={id} id={id}>
              <h3>{id}</h3>
              <ReasoningControlField {...fixture} thinkingEnabled={state[id].thinking_enabled}
                value={state[id].reasoning_control}
                onChange={patch => setState(current => ({ ...current, [id]: { ...current[id], ...patch } }))} />
            </section>)}
          </div>;
        }
        let root;
        const mount = () => { root = createRoot(document.getElementById('root')); root.render(<Harness />); };
        const check = (condition, message) => { if (!condition) throw new Error(message); };
        const settle = () => new Promise(resolve => setTimeout(resolve, 20));
        const wait = async predicate => {
          for (let attempt = 0; attempt < 200; attempt++) {
            if (predicate()) return;
            await settle();
          }
          throw new Error('Timed out waiting for: ' + predicate.toString());
        };
        const section = id => document.getElementById(id);
        const trigger = id => section(id)?.querySelector('.dropdown-trigger[aria-label="' + label('strength') + '"]');
        const key = (element, value) => element.dispatchEvent(new KeyboardEvent('keydown', {
          key: value, bubbles: true, cancelable: true,
        }));
        const choose = async (id, label) => {
          trigger(id).click();
          await wait(() => section(id).querySelector('[role=listbox]'));
          const option = [...section(id).querySelectorAll('[role=option]')]
            .find(element => element.textContent.trim() === label);
          check(option, id + ': requested option is available: ' + label);
          option.click();
          await wait(() => !section(id).querySelector('[role=listbox]'));
          await settle();
        };
        const label = key => i18next.t('settings.reasoning.' + key);
        const checkLayout = () => {
          check(document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1, 'No horizontal document overflow');
          for (const element of document.querySelectorAll('#root section, #root input, #root .dropdown-trigger, #root [role=listbox]')) {
            const rect = element.getBoundingClientRect();
            check(rect.left >= -1 && rect.right <= window.innerWidth + 1, 'Control stays within the viewport: ' + element.tagName);
          }
        };
        const setBudget = async value => {
          const input = section('budget').querySelector('input[type=number]');
          check(input, 'Budget field is visible');
          input.focus();
          Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, value);
          input.dispatchEvent(new Event('input', { bubbles: true }));
          await settle();
          input.blur();
          await settle();
          return input;
        };
        try {
          mount();
          await wait(() => document.querySelector('#mandatory .dropdown-trigger')
            && document.querySelector('#optional .dropdown-trigger')
            && document.querySelector('#budget .dropdown-trigger'));
          check(!section('mandatory').querySelector('[role=switch]'), 'Mandatory model has no off switch');
          check(!trigger('mandatory').disabled, 'Mandatory model remains adjustable when thinkingEnabled is false');
          await choose('mandatory', label('efforts.high'));
          check(settings.mandatory.reasoning_control.value === 'high', 'Mandatory model accepts effort');
          check(settings.mandatory.thinking_enabled === false, 'Effort selection preserves the stored thinking flag');

          const toggle = section('optional').querySelector('[role=switch]');
          toggle.click();
          await wait(() => toggle.getAttribute('aria-checked') === 'false' && !trigger('optional'));
          check(settings.optional.reasoning_control.value === 'high', 'Turning thinking off retains effort');
          toggle.click();
          await wait(() => toggle.getAttribute('aria-checked') === 'true' && trigger('optional'));
          check(trigger('optional').textContent.includes(label('efforts.high')), 'Turning thinking on restores effort');

          const keyboardTrigger = trigger('mandatory');
          keyboardTrigger.focus();
          key(keyboardTrigger, 'ArrowDown');
          await wait(() => section('mandatory').querySelector('[role=listbox]'));
          const options = [...section('mandatory').querySelectorAll('[role=option]')];
          await wait(() => document.activeElement === options.at(-1));
          key(document.activeElement, 'Home');
          check(document.activeElement === options[0], 'Home focuses the first effort option');
          key(document.activeElement, 'ArrowDown');
          check(document.activeElement === options[1], 'ArrowDown advances effort focus');
          key(document.activeElement, 'ArrowUp');
          check(document.activeElement === options[0], 'ArrowUp moves effort focus back');
          key(document.activeElement, 'End');
          check(document.activeElement === options.at(-1), 'End focuses the final effort option');
          checkLayout();
          key(document.activeElement, 'Escape');
          await wait(() => !section('mandatory').querySelector('[role=listbox]') && document.activeElement === keyboardTrigger);
          check(settings.mandatory.reasoning_control.value === 'high', 'Escape preserves effort and returns trigger focus');

          await choose('budget', label('budget'));
          await wait(() => section('budget').querySelector('input[type=number]')?.value === '128');
          const budgetInput = await setBudget('4096');
          check(settings.budget.reasoning_control.value === 4096, 'Valid budget is committed on blur');
          check(budgetInput.getAttribute('aria-invalid') === 'false', 'Valid budget has no validation error');
          for (const invalid of ['0', '127', '8193', '2.5', '']) {
            const input = await setBudget(invalid);
            check(input.getAttribute('aria-invalid') === 'true', 'Invalid budget is marked: ' + invalid);
            check(section('budget').querySelector('[role=alert]'), 'Invalid budget has an accessible error');
            check(settings.budget.reasoning_control.value === 4096, 'Invalid input preserves last valid saved budget');
          }
          await setBudget('8192');
          check(settings.budget.reasoning_control.value === 8192, 'Maximum budget boundary is accepted');
          await choose('budget', label('automatic'));
          check(settings.budget.reasoning_control.value === -1, 'Automatic budget saves the dynamic sentinel');
          check(!section('budget').querySelector('input'), 'Automatic budget hides numeric input');
          checkLayout();
          await wait(() => localStorage.getItem(storageKey) === JSON.stringify(settings));
          const saved = JSON.stringify(settings);
          root.unmount();
          mount();
          await wait(() => document.querySelector('#mandatory .dropdown-trigger')
            && document.querySelector('#optional .dropdown-trigger')
            && document.querySelector('#budget .dropdown-trigger'));
          check(JSON.stringify(settings) === saved, 'LocalStorage restores all controls after remount');
          check(trigger('mandatory').textContent.includes(label('efforts.high')), 'Mandatory effort is restored');
          check(section('optional').querySelector('[role=switch]').getAttribute('aria-checked') === 'true', 'Thinking toggle is restored');
          check(trigger('budget').textContent.includes(label('automatic')), 'Automatic budget is restored');
          await choose('budget', label('applicationDefault'));
          check(settings.budget.reasoning_control === null, 'Application default clears explicit control');
          checkLayout();
          root.unmount();

          let profile = { id: 'learning-profile', name: 'DeepSeek', provider: 'deepseek', region: 'old',
            capabilities: { supports_text_generation: true, supports_model_listing: true } };
          let loadedRegion;
          providersApi.apiProfiles = async () => ({ profiles: [profile] });
          providersApi.apiProfileModels = async () => {
            loadedRegion = profile.region;
            const model = profile.region === 'new' ? 'deepseek-v4-flash' : 'deepseek-flash';
            return { models: [model], model_details: {
              [model]: { reasoning: { supported: true, can_disable: true,
                supported_efforts: profile.region === 'new' ? ['low', 'high'] : ['low', 'high', 'max'] } },
            } };
          };
          updateLearningPreferences(current => ({ ...current, profileId: profile.id, model: 'deepseek-v4-flash',
            thinkingEnabled: false, reasoningControl: null }));
          const learningPreferences = () => readLearningPreferences(learningPreferencesSnapshot());
          const mountLearning = () => {
            root = createRoot(document.getElementById('root'));
            root.render(<section id="learning"><LearningAiSettingsSection /></section>);
          };
          mountLearning();
          await wait(() => loadedRegion === 'old' && section('learning')?.querySelector('[role=switch]'));
          check(!trigger('learning'), 'Learning hides strength while thinking is off');
          section('learning').querySelector('[role=switch]').click();
          await wait(() => learningPreferences().thinkingEnabled && trigger('learning'));
          trigger('learning').click();
          await wait(() => section('learning').querySelector('[role=listbox]'));
          check(JSON.stringify([...section('learning').querySelectorAll('[role=option]')].map(option => option.textContent.trim()))
            === JSON.stringify([label('applicationDefault'), ...['low', 'high', 'max'].map(value => label('efforts.' + value))]),
            'Saved DeepSeek alias exposes low, high and max even when only the current name is listed');
          key(document.activeElement, 'Escape');
          await wait(() => !section('learning').querySelector('[role=listbox]'));
          await choose('learning', label('efforts.max'));
          check(learningPreferences().reasoningControl?.value === 'max', 'Learning saves the selected effort');
          section('learning').querySelector('[role=switch]').click();
          await wait(() => !trigger('learning'));
          check(learningPreferences().reasoningControl?.value === 'max', 'Learning retains strength when thinking is off');
          section('learning').querySelector('[role=switch]').click();
          await wait(() => trigger('learning')?.textContent.includes(label('efforts.max')));
          checkLayout();
          root.unmount();
          mountLearning();
          await wait(() => trigger('learning')?.textContent.includes(label('efforts.max')));
          check(learningPreferences().thinkingEnabled, 'Learning restores thinking and strength after remount');

          profile = { ...profile, region: 'new' };
          notifyApiProfilesChanged();
          await wait(() => loadedRegion === 'new');
          await choose('learning', label('efforts.high'));
          check(learningPreferences().reasoningControl?.value === 'high', 'Learning accepts effort after a profile refresh');
          trigger('learning').click();
          await wait(() => section('learning').querySelector('[role=listbox]'));
          check(![...section('learning').querySelectorAll('[role=option]')]
            .some(option => option.textContent.trim() === label('efforts.max')), 'Refreshed catalog overrides the fallback');
          root.unmount();
          localStorage.removeItem('vrcs.learning.preferences.v1');
          localStorage.removeItem(storageKey);
          document.getElementById('result').textContent = 'PASS';
        } catch (error) { document.getElementById('result').textContent = 'FAIL: ' + error.message; }
      ` },
      plugins: [{ name: "locale-catalog-fixture", setup(build) {
        build.onResolve({ filter: /i18n\/catalog$/ }, () => ({ path: "locale-catalog", namespace: "fixture" }));
        build.onLoad({ filter: /.*/, namespace: "fixture" }, () => ({
          resolveDir: join(testDirectory, ".."),
          contents: 'import en from "./src/i18n/locales/en-US.json"; export const localeCatalog = [en];',
        }));
      } }],
      bundle: true, write: false, outfile: "reasoning-controls-test.js", format: "esm", platform: "browser", jsx: "automatic",
      define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
    });
    const directory = mkdtempSync(join(testDirectory, ".reasoning-controls-browser-"));
    try {
      const html = join(directory, "test.html");
      const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
      const stylesheet = bundle.outputFiles.find(file => file.path.endsWith(".css")).text;
      writeFileSync(html, '<meta name="viewport" content="width=device-width,initial-scale=1"><style>' + stylesheet
        + '</style><div id="root" class="settings-surface"></div><pre id="result">PENDING</pre><script type="module">'
        + javascript.replace(/<\/script/gi, "<\\/script") + '</script>');
      const result = spawnSync(browser, [
        "--headless=new", "--dump-dom", "--virtual-time-budget=15000", "--disable-gpu", "--no-first-run",
        "--disable-background-networking", "--disable-extensions", "--window-size=" + width + ",900",
        "--user-data-dir=" + join(directory, "profile"), pathToFileURL(html).href,
      ], { encoding: "utf8", timeout: 30000, maxBuffer: 8 * 1024 * 1024 });
      assert.ifError(result.error);
      assert.equal(result.status, 0, result.stderr);
      const report = result.stdout.match(/<pre id="result">([^<]*)<\/pre>/)?.[1];
      assert.equal(report, "PASS", report || result.stderr);
    } finally {
      rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
    }
  });
}
