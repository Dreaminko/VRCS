import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const testDirectory = dirname(fileURLToPath(import.meta.url));
const browser = [process.env.PROGRAMFILES, process.env["PROGRAMFILES(X86)"], process.env.LOCALAPPDATA]
  .filter(Boolean)
  .flatMap((root) => [
    join(root, "Google/Chrome/Application/chrome.exe"),
    join(root, "Microsoft/Edge/Application/msedge.exe"),
  ])
  .find(existsSync);

test("feature presets and custom switches commit visibility and roll back on save failure", {
  timeout: 45000,
  skip: !browser && "No installed Chrome or Edge executable was found.",
}, async () => {
  const resolveDir = join(testDirectory, "..");
  const bundle = await build({
    stdin: {
      resolveDir,
      loader: "tsx",
      contents: `
        import React from 'react';
        import { createRoot } from 'react-dom/client';
        import { flushSync } from 'react-dom';
        import i18next from 'i18next';
        import { initReactI18next } from 'react-i18next';
        import en from './src/i18n/locales/en-US.json';
        import { useSettingsRuntime } from './src/settings/hooks/useSettingsRuntime';
        import { useSettingsDraft } from './src/settings/hooks/useSettingsDraft';
        import { FeatureSettingsCard } from './src/settings/system/FeatureSettingsCard';
        import { SettingsTabBar } from './src/settings/components/SettingsTabBar';
        import { DEFAULT_FEATURE_SETTINGS, visibleSettingsCategories } from './src/settings/feature-availability';
        import { updateReducedMotionPreference } from './src/app/app-motion';
        import './src/styles.css';

        await i18next.use(initReactI18next).init({
          lng: 'en-US',
          resources: { 'en-US': { translation: en.translation } },
        });
        updateReducedMotionPreference(true);

        let persisted = {
          features: { ...DEFAULT_FEATURE_SETTINGS },
          audio: { output: { mode: 'disabled' }, microphone: { mode: 'disabled', device_id: null } },
        };
        let putCount = 0;
        let pendingSave = null;
        window.fetch = async (_url, init) => {
          if (init?.method === 'PUT') {
            putCount += 1;
            const body = init.body;
            return await new Promise((resolve) => {
              pendingSave = (fail) => {
                pendingSave = null;
                if (!fail) persisted = JSON.parse(body);
                resolve(new Response(
                  JSON.stringify(fail ? { code: 'settings.invalid', detail: 'Rejected for test', params: {} } : persisted),
                  { status: fail ? 422 : 200 },
                ));
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
            <FeatureSettingsCard
              features={controller.draft.features}
              disabled={false}
              onUpdate={(update, afterError) => controller.applySettings(
                (current) => ({ ...current, features: update(current.features) }),
                undefined,
                afterError,
              )}
            />
            {controller.saveMessage && <p role="alert">{controller.saveMessage}</p>}
          </div>;
        }
        function Harness() {
          const runtime = useSettingsRuntime({
            active: false,
            coreConfigured: true,
            health,
            stopMicrophoneTest: async () => {},
            setCaptureRunning: async () => true,
            playerAudioStart: { confirm: async () => true, ensurePrivileges: async () => true },
            clearErrorFrom: noop,
            reportError: noop,
          });
          return runtime.value ? <Features settings={runtime.value} save={runtime.save} /> : null;
        }

        const root = createRoot(document.getElementById('root'));
        root.render(<Harness />);

        const check = (condition, message) => { if (!condition) throw new Error(message); };
        const settle = () => new Promise((resolve) => setTimeout(resolve, 16));
        const wait = async (predicate, label) => {
          for (let attempt = 0; attempt < 250; attempt += 1) {
            if (predicate()) return;
            await settle();
          }
          throw new Error('Timed out waiting for: ' + label);
        };
        const findButton = (text) =>
          [...document.querySelectorAll('.system-features-mode-group button')]
            .find((el) => el.textContent?.trim() === text);
        const findSwitch = (label) =>
          document.querySelector('.system-features-group [role="switch"][aria-label="' + label + '"]');
        const findTab = (category) =>
          document.querySelector('[role="tab"][data-settings-category="' + category + '"]');
        const completeSave = async (fail) => {
          await wait(() => typeof pendingSave === 'function', 'pending PUT request');
          pendingSave(fail);
        };
        const click = (el) => { check(el, 'Element not found for click'); flushSync(() => el.click()); };

        try {
          await wait(() => Boolean(findSwitch('Dictionary and learning')), 'initial custom switches');

          const translationOnly = findButton('Translation only');
          const allFeatures = findButton('All features');
          const custom = findButton('Custom');
          check(custom.getAttribute('aria-pressed') === 'true', 'default settings start in Custom');
          check(translationOnly.getAttribute('aria-pressed') === 'false', 'Translation only starts unselected');
          check(allFeatures.getAttribute('aria-pressed') === 'false', 'All features starts unselected');
          check(!findSwitch('OSC Chatbox'), 'OSC Chatbox is not an optional feature switch');
          check(!findSwitch('Microphone speaker blocking'), 'Microphone speaker blocking is not an optional feature switch');
          check(!('microphone_speaker_filter' in persisted.features), 'microphone_speaker_filter is not present');

          // 1. Failed individual switch rolls back and shows alert.
          const vqaSwitch = findSwitch('Visual question answering');
          check(vqaSwitch.getAttribute('aria-checked') === 'false', 'vqa starts off');
          click(vqaSwitch);
          check(vqaSwitch.getAttribute('aria-checked') === 'true', 'vqa toggles optimistically');
          await completeSave(true);
          await wait(() => Boolean(document.querySelector('[role="alert"]')), 'save failure alert');
          await wait(() => findSwitch('Visual question answering')?.getAttribute('aria-checked') === 'false', 'vqa rollback');

          // 2. Enabling the last remaining switch persists and keeps Custom expanded rather than collapsing mid-edit.
          click(findSwitch('Visual question answering'));
          await completeSave(false);
          await wait(() => persisted.features.visual_question_answering === true, 'vqa persisted');
          check(!('osc_chatbox' in persisted.features), 'osc_chatbox is not present');
          check(!('microphone_speaker_filter' in persisted.features), 'microphone_speaker_filter is not present');
          check(custom.getAttribute('aria-pressed') === 'true', 'stays in Custom while editing');
          check(Boolean(findSwitch('Dictionary and learning')), 'switches stay mounted while editing');

          // 4. Clicking All features when already matching collapses without sending a redundant PUT.
          const putsBeforeRedundantFull = putCount;
          click(allFeatures);
          check(allFeatures.getAttribute('aria-pressed') === 'true', 'All features selected');
          await wait(() => !findSwitch('Dictionary and learning'), 'switches collapsed in All features');
          check(putCount === putsBeforeRedundantFull, 'no redundant PUT when features already match All features');

          // 5. Clicking Custom expands switches without sending a PUT.
          click(custom);
          await wait(() => Boolean(findSwitch('Dictionary and learning')), 'switches expanded in Custom');
          check(custom.getAttribute('aria-pressed') === 'true', 'Custom selected');
          check(putCount === putsBeforeRedundantFull, 'opening Custom does not send PUT');

          // 6. Manually disable VR Overlay so we can verify Translation only preserves vr_overlay: false.
          click(findSwitch('VR Overlay'));
          await completeSave(false);
          await wait(() => persisted.features.vr_overlay === false, 'vr_overlay persisted false');

          // 7. Switch to Translation only: single atomic PUT, preserves vr_overlay: false, keeps glossary: true.
          const putsBeforeTranslation = putCount;
          click(translationOnly);
          check(translationOnly.getAttribute('aria-pressed') === 'true', 'Translation only selected');
          await completeSave(false);
          check(putCount === putsBeforeTranslation + 1, 'Translation only uses a single atomic PUT');
          await wait(() => !findTab('learning') && !findTab('vision') && !findTab('vr_overlay') && Boolean(findTab('glossary')), 'tabs updated for Translation only');
          await wait(() => !findSwitch('Dictionary and learning'), 'switches collapsed in Translation only');
          const expectedTranslationOnly = {
            glossary: true,
            learning: false,
            anki: false,
            vrcx: false,
            ocr: false,
            visual_question_answering: false,
            vr_overlay: false,
            external_api: false,
          };
          check(
            Object.entries(expectedTranslationOnly).every(([key, value]) => persisted.features[key] === value)
              && Object.keys(persisted.features).length === Object.keys(expectedTranslationOnly).length,
            'Translation only persisted expected flags: ' + JSON.stringify(persisted.features),
          );

          // 8. Open Custom while in Translation only, fail switch to All features -> rolls back to Custom + previous values.
          click(custom);
          await wait(() => Boolean(findSwitch('Dictionary and learning')), 'switches expanded from Translation only');
          check(findSwitch('VR Overlay').getAttribute('aria-checked') === 'false', 'VR Overlay remains false in Custom');
          check(findSwitch('Glossary').getAttribute('aria-checked') === 'true', 'Glossary remains true in Custom');

          click(allFeatures);
          await completeSave(true);
          await wait(() => Boolean(document.querySelector('[role="alert"]')), 'preset failure alert');
          await wait(() => custom.getAttribute('aria-pressed') === 'true' && Boolean(findSwitch('Dictionary and learning')), 'rolls back to Custom');
          check(findSwitch('Dictionary and learning').getAttribute('aria-checked') === 'false', 'learning switch rolled back');

          // 9. Succeed switching to All features -> enables all 8 switches including vr_overlay.
          const putsBeforeFull = putCount;
          click(allFeatures);
          await completeSave(false);
          check(putCount === putsBeforeFull + 1, 'All features uses a single atomic PUT');
          await wait(() => Boolean(findTab('learning')) && Boolean(findTab('vision')) && Boolean(findTab('vr_overlay')) && Boolean(findTab('glossary')), 'all tabs restored');
          await wait(() => !findSwitch('Dictionary and learning'), 'switches collapsed after All features');
          check(Object.values(persisted.features).every(Boolean), 'all 8 features enabled in All features');

          // 10. Switch to Translation only from All features -> preserves vr_overlay: true.
          click(translationOnly);
          await completeSave(false);
          await wait(() => !findTab('learning') && Boolean(findTab('vr_overlay')) && Boolean(findTab('glossary')), 'VR Overlay and Glossary stay visible');
          check(persisted.features.vr_overlay === true, 'vr_overlay stays true when previously enabled');
          check(persisted.features.glossary === true, 'glossary stays true in Translation only');

          // 11. Layout check: mode toggle buttons do not overflow the card.
          const cardRect = document.querySelector('.system-features-group').getBoundingClientRect();
          const groupRect = document.querySelector('.system-features-mode-group').getBoundingClientRect();
          check(groupRect.right <= cardRect.right + 1, 'mode buttons fit within feature card');

          document.getElementById('result').textContent = 'PASS';
        } catch (error) {
          document.getElementById('result').textContent = 'FAIL: ' + (error?.stack || String(error));
        }
      `,
    },
    bundle: true,
    write: false,
    outfile: "feature-switches-test.js",
    format: "esm",
    platform: "browser",
    jsx: "automatic",
    define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
  });

  const directory = mkdtempSync(join(testDirectory, ".feature-switches-browser-"));
  try {
    const html = join(directory, "test.html");
    const javascript = bundle.outputFiles.find((file) => file.path.endsWith(".js")).text;
    const stylesheet = bundle.outputFiles.find((file) => file.path.endsWith(".css")).text;
    writeFileSync(
      html,
      `<style>${stylesheet}</style><div id="root" style="max-width:1080px;margin:24px auto"></div><pre id="result">PENDING</pre><script type="module">${javascript.replace(/<\/script/gi, "<\\/script")}</script>`,
    );
    const result = spawnSync(
      browser,
      [
        "--headless=new",
        "--no-sandbox",
        "--dump-dom",
        "--virtual-time-budget=12000",
        "--disable-gpu",
        "--no-first-run",
        "--disable-background-networking",
        "--disable-extensions",
        "--window-size=1200,900",
        "--user-data-dir=" + join(directory, "profile"),
        pathToFileURL(html).href,
      ],
      { encoding: "utf8", timeout: 30000, maxBuffer: 8 * 1024 * 1024 },
    );
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    const report = result.stdout.match(/<pre id="result">([^<]*)<\/pre>/)?.[1];
    assert.equal(report, "PASS", report || result.stderr);
  } finally {
    rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
});
