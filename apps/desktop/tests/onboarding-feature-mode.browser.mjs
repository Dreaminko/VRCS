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

for (const width of [1200, 380]) {
  test(`onboarding lets users choose feature modes with explanations and rolls back failures at ${width}px`, {
    timeout: 45000,
    skip: !browser && "No installed Chrome or Edge executable was found.",
  }, async () => {
    const resolveDir = join(testDirectory, "..");
    const bundle = await build({
      stdin: {
        resolveDir,
        loader: "tsx",
        contents: `
          import React, { useState } from 'react';
          import { createRoot } from 'react-dom/client';
          import { flushSync } from 'react-dom';
          import i18next from 'i18next';
          import { I18nextProvider, initReactI18next } from 'react-i18next';
          import en from './src/i18n/locales/en-US.json';
          import { useSettingsRuntime } from './src/settings/hooks/useSettingsRuntime';
          import { OnboardingWizard } from './src/onboarding/OnboardingWizard';
          import { DEFAULT_FEATURE_SETTINGS } from './src/settings/feature-availability';
          import { updateReducedMotionPreference } from './src/app/app-motion';
          import './src/styles.css';

          await i18next.use(initReactI18next).init({
            lng: 'en-US',
            resources: { 'en-US': { translation: en.translation } },
            interpolation: { escapeValue: false },
          });
          updateReducedMotionPreference(true);

          let persisted = {
            schema_version: 45,
            features: { ...DEFAULT_FEATURE_SETTINGS },
            storage: {
              database_path: 'data/vrcs.db',
              model_directory: 'models/asr',
              subtitle_history_max_bytes: 10485760,
              model_download_source: 'auto',
            },
            audio: {
              sample_rate: 16000,
              output: { mode: 'system', device_id: 1 },
              microphone: { mode: 'disabled', device_id: null },
            },
            asr: {
              backend: 'qwen_local_managed',
              active_profile_id: null,
              language: 'auto',
              managed_qwen: { package_id: 'qwen3-asr-0.6b-q8_0', device: 'auto' },
              service_settings: {},
            },
          };
          let putCount = 0;
          const pendingSaves = [];

          window.fetch = async (url, init) => {
            const target = String(url);
            if (init?.method === 'PUT') {
              putCount += 1;
              const body = init.body;
              return await new Promise((resolve) => {
                pendingSaves.push((fail) => {
                  if (!fail) persisted = JSON.parse(body);
                  resolve(new Response(
                    JSON.stringify(fail ? { code: 'settings.invalid', detail: 'Rejected for test', params: {} } : persisted),
                    { status: fail ? 422 : 200 },
                  ));
                });
              });
            }
            if (target.includes('/api/providers')) {
              return new Response(JSON.stringify({ providers: [] }), { status: 200 });
            }
            if (target.includes('/api/profile')) {
              return new Response(JSON.stringify({ profiles: [] }), { status: 200 });
            }
            return new Response(JSON.stringify(persisted), { status: 200 });
          };

          const noop = () => {};
          const noopAsync = async () => {};
          const health = { getCurrent: () => null, refreshQuietly: noopAsync };
          let setWizardInstance = null;

          function Harness() {
            const [instance, setInstance] = useState({ key: 'welcome', step: 0 });
            setWizardInstance = setInstance;
            const runtime = useSettingsRuntime({
              active: false,
              coreConfigured: true,
              health,
              stopMicrophoneTest: noopAsync,
              setCaptureRunning: async () => true,
              playerAudioStart: { confirm: async () => true, ensurePrivileges: async () => true },
              clearErrorFrom: noop,
              reportError: noop,
            });
            if (!runtime.value) return null;
            return (
              <I18nextProvider i18n={i18next}>
                <div className="app-shell onboarding-shell">
                  <div className="window-chrome" />
                  <OnboardingWizard
                    key={instance.key}
                    initialStep={instance.step}
                    settings={runtime.value}
                    health={null}
                    devices={[
                      { id: 1, name: 'Speakers', is_loopback: true },
                      { id: 2, name: 'Mic', is_loopback: false },
                    ]}
                    devicesReady={true}
                    onRefreshDevices={noopAsync}
                    onRefreshSettings={runtime.refresh}
                    onModelsChanged={noopAsync}
                    onStartMicrophoneTest={noopAsync}
                    onStopMicrophoneTest={noopAsync}
                    onSave={runtime.save}
                    onUpdateSettings={runtime.update}
                    onProgress={noopAsync}
                    onSkip={noopAsync}
                    onComplete={async () => {}}
                  />
                </div>
              </I18nextProvider>
            );
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
          const completeNextSave = async (fail) => {
            await wait(() => pendingSaves.length > 0, 'pending PUT request');
            const finish = pendingSaves.shift();
            finish(fail);
          };
          const click = (el) => { check(el, 'Element not found for click'); flushSync(() => el.click()); };
          const findModeButton = (label) =>
            [...document.querySelectorAll('.onboarding-feature-choice')]
              .find((el) => el.querySelector('strong')?.textContent?.trim() === label);
          const findSwitch = (label) =>
            document.querySelector('.onboarding-feature-toggles [role="switch"][aria-label="' + label + '"]');
          const assertFitsViewport = (stage) => {
            check(
              document.documentElement.scrollWidth <= innerWidth,
              stage + ': horizontal overflow ' + document.documentElement.scrollWidth + ' > ' + innerWidth,
            );
          };

          let stage = 'initial mount';
          try {
            await wait(() => Boolean(findSwitch('Dictionary and learning')), 'initial welcome switches');
            for (const animation of document.getAnimations()) animation.finish();
            await settle();
            check(putCount === 0, 'mounting welcome step does not send PUT');
            assertFitsViewport(stage);

            const surface = document.querySelector('.onboarding-surface');
            const sidebar = document.querySelector('.onboarding-sidebar');
            const scrollRegion = document.querySelector('.onboarding-scroll-region');
            const content = document.querySelector('.onboarding-content');
            const actions = document.querySelector('.onboarding-actions');
            await wait(() => Math.abs(surface.getBoundingClientRect().height - surface.offsetHeight) <= 0.5, 'surface entrance animation settled');
            const expandedSurfaceHeight = surface.getBoundingClientRect().height;
            const expandedSidebarHeight = sidebar.getBoundingClientRect().height;
            if (innerWidth > 680) {
              check(expandedSurfaceHeight <= 680.5, 'surface stays bounded at <= 680px when Custom is expanded: ' + expandedSurfaceHeight);
              check(Math.abs(expandedSidebarHeight - surface.clientHeight) <= 1, 'sidebar matches surface inner height when expanded: ' + expandedSidebarHeight + ' vs ' + surface.clientHeight);
            } else {
              check(expandedSurfaceHeight <= scrollRegion.clientHeight + 1, 'narrow surface fits inside scroll region: ' + expandedSurfaceHeight + ' vs ' + scrollRegion.clientHeight);
            }
            check(getComputedStyle(content).overflowY === 'auto', 'right content area is an independent scroll viewport');
            check(content.scrollHeight > content.clientHeight, 'expanded Custom overflows inside right content viewport');
            check(scrollRegion.scrollHeight <= scrollRegion.clientHeight + 1, 'outer scroll region does not scroll when content overflows');
            check(actions.getBoundingClientRect().bottom <= surface.getBoundingClientRect().bottom + 1, 'actions footer remains pinned inside surface');

            const languageSetting = document.querySelector('.onboarding-language-setting:not(.onboarding-download-setting)');
            const downloadSetting = document.querySelector('.onboarding-download-setting');
            const featureSection = document.querySelector('.onboarding-feature-section');
            check(Boolean(languageSetting && downloadSetting && featureSection), 'welcome settings sections are present');
            check(downloadSetting.classList.contains('onboarding-language-setting'), 'download source reuses language setting card styling');
            check(Boolean(languageSetting.compareDocumentPosition(downloadSetting) & Node.DOCUMENT_POSITION_FOLLOWING), 'download source appears below UI language');
            check(Boolean(downloadSetting.compareDocumentPosition(featureSection) & Node.DOCUMENT_POSITION_FOLLOWING), 'feature mode appears below download source');

            const translationOnly = findModeButton('Translation only');
            const allFeatures = findModeButton('All features');
            const custom = findModeButton('Custom');
            check(custom.getAttribute('aria-pressed') === 'true', 'DEFAULT_FEATURE_SETTINGS shows Custom');
            check(document.querySelectorAll('.onboarding-feature-detail').length === 0, 'detailed mode descriptions are omitted');
            check(translationOnly.textContent.includes('Focus on live captions and translation'), 'Translation only shows concise summary');
            check(allFeatures.textContent.includes('Enable all features'), 'All features shows concise summary');
            check(custom.textContent.includes('Pick individual modules'), 'Custom shows concise summary');
            if (innerWidth > 680) {
              for (const choice of document.querySelectorAll('.onboarding-feature-choice')) {
                const height = choice.getBoundingClientRect().height;
                check(height < 80, 'feature mode choice stays compact (< 80px), got: ' + height);
              }
            }
            check(document.querySelector('.onboarding-feature-footnote')?.textContent.includes('Settings › System › Features'), 'footnote points to System settings');

            // 1. Disable VR Overlay manually, then apply Translation only after changing download source.
            stage = 'disable vr_overlay';
            click(findSwitch('VR Overlay'));
            await completeNextSave(false);
            await wait(() => persisted.features.vr_overlay === false && !findSwitch('VR Overlay').disabled, 'vr_overlay saved false');

            stage = 'translation_only preserves download source';
            const putsBeforeTranslation = putCount;
            const downloadTrigger = document.querySelector('.download-source-settings .dropdown-trigger');
            click(downloadTrigger);
            await wait(() => Boolean(document.querySelector('.download-source-settings [role="option"]')), 'download source options');
            const modelscopeOption = [...document.querySelectorAll('.download-source-settings [role="option"]')]
              .find((el) => el.textContent.includes('ModelScope'));
            click(modelscopeOption);
            await completeNextSave(false);
            await wait(() => persisted.storage.model_download_source === 'modelscope' && !translationOnly.disabled, 'download source saved');
            click(translationOnly);
            await completeNextSave(false);
            check(putCount === putsBeforeTranslation + 2, 'download source and translation_only each send 1 PUT');
            await wait(() => !findSwitch('Dictionary and learning') && !translationOnly.disabled, 'switches collapse in Translation only');
            check(translationOnly.getAttribute('aria-pressed') === 'true', 'Translation only is pressed');
            check(persisted.storage.model_download_source === 'modelscope', 'download source change is preserved');
            check(persisted.features.glossary === true, 'glossary remains enabled in Translation only');
            check(persisted.features.vr_overlay === false, 'vr_overlay stays false when disabled');
            check(persisted.features.learning === false && persisted.features.anki === false && persisted.features.ocr === false, 'optional modules disabled');
            const collapsedSurfaceHeight = surface.getBoundingClientRect().height;
            check(Math.abs(collapsedSurfaceHeight - expandedSurfaceHeight) <= 1, 'collapsing switches keeps surface height fixed: ' + collapsedSurfaceHeight + ' vs ' + expandedSurfaceHeight);
            if (innerWidth > 680) {
              const collapsedSidebarHeight = sidebar.getBoundingClientRect().height;
              check(Math.abs(collapsedSidebarHeight - expandedSidebarHeight) <= 1, 'collapsing right panel keeps sidebar height fixed: ' + collapsedSidebarHeight + ' vs ' + expandedSidebarHeight);
            }
            check(actions.getBoundingClientRect().bottom <= surface.getBoundingClientRect().bottom + 1, 'actions footer remains pinned inside surface after collapse');
            assertFitsViewport(stage);

            // 2. CompleteStep reflects Translation only (shows Feature mode & Glossary, hides Learning).
            stage = 'complete step translation_only';
            flushSync(() => setWizardInstance({ key: 'complete-translation', step: 4 }));
            await wait(() => Boolean(document.querySelector('.onboarding-complete')), 'complete step rendered');
            const completeText = document.querySelector('.onboarding-complete').textContent;
            check(completeText.includes('Feature mode') && completeText.includes('Translation only'), 'CompleteStep shows Translation only mode');
            const badges = [...document.querySelectorAll('.onboarding-next-hints > div span')].map((el) => el.textContent.trim());
            check(badges.includes('Glossaries'), 'CompleteStep shows Glossaries badge');
            check(!badges.includes('Learning'), 'CompleteStep hides Learning badge in Translation only');
            check(!badges.includes('VR Overlay'), 'CompleteStep hides VR Overlay badge when disabled');
            assertFitsViewport(stage);

            // 3. Return to Welcome, expand Custom (0 PUTs), toggle a switch (stays expanded), fail All features -> rolls back.
            stage = 'custom and rollback';
            flushSync(() => setWizardInstance({ key: 'welcome-2', step: 0 }));
            await wait(() => Boolean(findModeButton('Custom')), 'welcome remounted');
            const putsBeforeCustom = putCount;
            click(findModeButton('Custom'));
            await wait(() => Boolean(findSwitch('Dictionary and learning')), 'switches expand in Custom');
            check(putCount === putsBeforeCustom, 'opening Custom sends 0 PUTs');
            assertFitsViewport(stage);

            click(findSwitch('Anki'));
            await completeNextSave(false);
            await wait(() => persisted.features.anki === true && !findSwitch('Anki').disabled, 'individual switch saves immediately');
            check(findModeButton('Custom').getAttribute('aria-pressed') === 'true', 'stays in Custom after toggling switch');
            check(Boolean(findSwitch('Dictionary and learning')), 'switches remain expanded after toggling switch');

            click(findModeButton('All features'));
            await completeNextSave(true);
            await wait(() => Boolean(document.querySelector('[role="alert"]')), 'save failure alert');
            await wait(() => findModeButton('Custom').getAttribute('aria-pressed') === 'true' && Boolean(findSwitch('Dictionary and learning')) && !findModeButton('All features').disabled, 'rolls back to Custom');
            check(persisted.features.learning === false && persisted.features.anki === true, 'persisted features unchanged after failed save');

            // 4. Succeed switching to All features -> all 8 enabled, collapses switches.
            stage = 'all features';
            const putsBeforeFull = putCount;
            click(findModeButton('All features'));
            await completeNextSave(false);
            check(putCount === putsBeforeFull + 1, 'All features sends 1 atomic PUT');
            await wait(() => !findSwitch('Dictionary and learning') && !findModeButton('All features').disabled, 'switches collapse in All features');
            check(Object.values(persisted.features).every(Boolean), 'all 8 features enabled');
            assertFitsViewport(stage);

            // 5. CompleteStep reflects All features (shows Learning & VR Overlay badges).
            stage = 'complete step full';
            flushSync(() => setWizardInstance({ key: 'complete-full', step: 4 }));
            await wait(() => Boolean(document.querySelector('.onboarding-complete')), 'complete step full rendered');
            const fullBadges = [...document.querySelectorAll('.onboarding-next-hints > div span')].map((el) => el.textContent.trim());
            check(document.querySelector('.onboarding-complete').textContent.includes('All features'), 'CompleteStep shows All features mode');
            check(fullBadges.includes('Learning') && fullBadges.includes('VR Overlay') && fullBadges.includes('Glossaries'), 'CompleteStep shows all enabled badges');

            // 6. Return to Welcome from All features and switch to Translation only -> preserves vr_overlay: true.
            stage = 'translation_only preserves vr_overlay true';
            flushSync(() => setWizardInstance({ key: 'welcome-3', step: 0 }));
            await wait(() => Boolean(findModeButton('Translation only')), 'welcome remounted for vr_overlay check');
            click(findModeButton('Translation only'));
            await completeNextSave(false);
            await wait(() => persisted.features.learning === false && !findModeButton('Translation only').disabled, 'translation_only saved');
            check(persisted.features.vr_overlay === true, 'vr_overlay stays true when previously enabled');
            check(persisted.features.glossary === true, 'glossary stays true');
            assertFitsViewport(stage);

            document.getElementById('result').textContent = 'PASS';
          } catch (error) {
            document.getElementById('result').textContent = 'FAIL (' + stage + '): ' + (error?.stack || String(error));
          }
          parent.postMessage(document.getElementById('result').textContent, '*');
        `,
      },
      plugins: [{
        name: "locale-fixture",
        setup(build) {
          build.onLoad({ filter: /[\\/]i18n[\\/]catalog\.ts$/ }, () => ({
            contents: 'import en from "./src/i18n/locales/en-US.json"; export const localeCatalog = [en]; export const supportedUiLocales = ["en-US"]; export const localeResources = { "en-US": { translation: en.translation } };',
            resolveDir: join(testDirectory, ".."),
            loader: "ts",
          }));
        },
      }],
      bundle: true,
      write: false,
      outfile: "onboarding-feature-mode-test.js",
      format: "esm",
      platform: "browser",
      jsx: "automatic",
      define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
    });

    const directory = mkdtempSync(join(testDirectory, ".onboarding-feature-mode-browser-"));
    try {
      const javascript = bundle.outputFiles.find((file) => file.path.endsWith(".js")).text;
      const stylesheet = bundle.outputFiles.find((file) => file.path.endsWith(".css")).text;
      const html = join(directory, "test.html");
      writeFileSync(
        join(directory, "content.html"),
        `<style>${stylesheet}</style><div id="root"></div><pre id="result" hidden>PENDING</pre>`
        + `<script>const fail = message => { document.getElementById("result").textContent = "FAIL: " + message; parent.postMessage("FAIL: " + message, "*"); }; window.addEventListener("error", event => fail(event.message)); window.addEventListener("unhandledrejection", event => fail(String(event.reason)));</script>`
        + `<script type="module">${javascript.replace(/<\/script/gi, "<\\/script")}</script>`,
      );
      writeFileSync(
        html,
        `<body style="margin:0"><iframe src="content.html" style="border:0;width:${width}px;height:760px"></iframe><pre id="result">PENDING</pre><script>window.addEventListener("message", event => document.getElementById("result").textContent = event.data);</script></body>`,
      );
      const result = spawnSync(
        browser,
        [
          "--headless=new",
          "--no-sandbox",
          "--dump-dom",
          "--virtual-time-budget=15000",
          "--disable-gpu",
          "--no-first-run",
          "--disable-background-networking",
          "--disable-extensions",
          "--window-size=" + Math.max(500, width) + ",1000",
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
}
