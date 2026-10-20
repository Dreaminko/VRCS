import assert from "node:assert/strict";
import { spawn } from "node:child_process";
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

for (const [width, systemReduced] of [[1200, false], [380, false], [1200, true]]) {
  test(`settings animate disclosures and honor the saved motion preference at ${width}px (system reduced: ${systemReduced})`, {
    timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
  }, async () => {
    const bundle = await build({
      stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
        import React, { useState } from 'react';
        import { createRoot } from 'react-dom/client';
        import { flushSync } from 'react-dom';
        import i18next from 'i18next';
        import { initReactI18next } from 'react-i18next';
        import en from './src/i18n/locales/en-US.json';
        import { SystemSettingsSection } from './src/settings/sections/SystemSettingsSection';
        import { VisionSettingsCard } from './src/settings/sections/VisionSettingsCard';
        import { DEFAULT_VISION_SETTINGS } from './src/settings/vision-settings';
        import { initializeMotionPreference, readReducedMotionPreference } from './src/app/app-motion';
        import { updateThemePreference } from './src/app/app-theme';
        import './src/styles.css';
        await i18next.use(initReactI18next).init({ lng: 'en-US', resources: { 'en-US': { translation: en.translation } } });
        window.requestAnimationFrame = callback => window.setTimeout(() => callback(performance.now()), 16);
        window.cancelAnimationFrame = id => window.clearTimeout(id);
        window.fetch = async () => new Response('{}', { status: 200 });
        const stopMotion = initializeMotionPreference();
        let themeTransitions = 0;
        Object.defineProperty(document, 'startViewTransition', { configurable: true, value: callback => {
          themeTransitions++; callback(); return { finished: Promise.resolve() };
        } });
        const noop = () => {};
        function Harness() {
          const [vision, setVision] = useState(DEFAULT_VISION_SETTINGS);
          return <>
            <SystemSettingsSection desktopPreferences={{ launchAtStartup: false, minimizeToTray: false }}
              desktopPreferencesReady={true} desktopSaveState="saved" desktopMessage="" uiLanguagePreference="en-US"
              interfaceScale={100} onUpdateDesktop={async () => {}} onInterfaceScaleChange={noop}
              onUpdateUiLanguage={async () => {}} onboardingDisabled={false} onStartOnboarding={noop}
              locale="en-US" draft={{ features: {}, conversation_titles: { auto_generate: false, profile_id: null, model: '', output_language: 'auto' }, storage: { subtitle_history_max_bytes: 10485760 } }}
              profiles={[]} onManageProfiles={noop} applySettings={noop} updater={{ phase: 'idle', automaticChecks: false }} />
            <VisionSettingsCard config={vision} profiles={[]} disabled={false} vrAvailable={true}
              onChange={patch => setVision(current => ({ ...current, ...patch }))} onSelectProfile={noop} />
          </>;
        }
        const root = createRoot(document.getElementById('root'));
        flushSync(() => root.render(<Harness />));
        const check = (condition, message) => { if (!condition) throw new Error(message); };
        const settle = () => new Promise(resolve => setTimeout(resolve, 20));
        const wait = async predicate => {
          for (let i = 0; i < 150; i++) { if (predicate()) return; await settle(); }
          throw new Error('Timed out: ' + predicate.toString());
        };
        const click = element => flushSync(() => element.click());
        const height = element => element.getBoundingClientRect().height;
        const finish = async element => {
          for (const animation of element.getAnimations({ subtree: true })) animation.finish();
          await settle();
        };
        const pauseAt = (element, fraction) => {
          const animations = element.getAnimations({ subtree: true });
          for (const animation of animations) {
            animation.pause(); animation.currentTime = Number(animation.effect.getTiming().duration) * fraction;
          }
          return animations;
        };
        try {
          await settle();
          const groups = [...document.querySelectorAll('.system-settings-list > .system-settings-group:not(.software-update-settings)')];
          check(groups.length === 4, 'All system settings groups are available');
          check(document.querySelector('.system-settings-list > :last-child').classList.contains('software-update-settings'),
            'Software updates appear last in system settings');
          for (const group of groups) {
            const control = group.querySelector('button, input');
            check(control?.checkVisibility(), 'System settings are visible without expanding a group: ' + group.getAttribute('aria-labelledby'));
            click(group.querySelector(':scope > .section-heading'));
            check(control.checkVisibility(), 'Clicking a system settings heading does not hide its controls');
          }
          const card = document.querySelector('.vision-settings-card');
          const master = card.querySelector('[role=switch]');
          const collapsedHeight = height(card);
          click(master);
          let region = card.querySelector(':scope > .settings-disclosure');
          check(region && !region.inert, 'Master switch opens an interactive settings region');
          check(region.getAnimations().length === (${systemReduced} ? 0 : 1), 'Disclosure respects the system motion preference');
          await finish(region);
          const expandedHeight = height(card);
          check(expandedHeight > collapsedHeight + 100, 'Enabling restores the full settings content');
          check(getComputedStyle(region).overflow === 'visible', 'Expanded content permits dropdowns to extend beyond it');
          click(master);
          if (!${systemReduced}) {
            check(region.isConnected && region.inert && region.getAttribute('aria-hidden') === 'true', 'Closing content is immediately unavailable to input');
            pauseAt(region, 0.5);
            const midHeight = height(card);
            check(midHeight > collapsedHeight && midHeight < expandedHeight, 'Closing has an intermediate layout height');
            const before = height(region);
            click(master);
            pauseAt(region, 0);
            check(Math.abs(height(region) - before) <= 1, 'Reversing a closing disclosure keeps its current height');
            await finish(region);
            click(master);
            pauseAt(region, 1);
            check(Math.abs(height(card) - collapsedHeight) <= 1, 'Final animation frame already has the collapsed spacing');
            await finish(region);
          }
          check(!region.isConnected && Math.abs(height(card) - collapsedHeight) <= 1, 'Removing closed content leaves no extra gap');
          click(master);
          region = card.querySelector(':scope > .settings-disclosure');
          if (!${systemReduced}) {
            pauseAt(region, 0.5);
            check(height(card) > collapsedHeight + (expandedHeight - collapsedHeight) * 0.6
              && height(card) < expandedHeight, 'Opening slows toward its full height');
          }
          await finish(region);
          const disclosures = [...card.querySelectorAll('.vision-settings-details')];
          check(disclosures.length === 3, 'System prompt, limits and wrist position are available');
          for (const details of disclosures) {
            const closedHeight = height(details);
            getComputedStyle(details, '::details-content').blockSize;
            click(details.querySelector('summary'));
            const content = details.querySelector(':scope > :not(summary)');
            const openedHeight = closedHeight + height(content) + parseFloat(getComputedStyle(content).marginTop);
            // Chromium does not expose native details-content transitions through getAnimations().
            if (!${systemReduced}) await wait(() => height(details) > closedHeight + 1 && height(details) < openedHeight - 1);
            await wait(() => Math.abs(height(details) - openedHeight) <= 0.1);
            check(openedHeight > closedHeight + 12, 'Expanded details include their content and spacing');
            click(details.querySelector('summary'));
            getComputedStyle(details, '::details-content').blockSize;
            if (!${systemReduced}) await wait(() => height(details) > closedHeight + 1 && height(details) < openedHeight - 1);
            await wait(() => Math.abs(height(details) - closedHeight) <= 0.1);
          }
          const general = document.querySelector('[aria-labelledby=system-general-title]');
          const reduce = general.querySelector('button[aria-label="Reduce motion"]');
          check(reduce && reduce.getAttribute('aria-checked') === 'false', 'General settings expose the app preference');
          if (!${systemReduced}) {
            click(master);
            pauseAt(region, 0.5);
          }
          click(reduce);
          await settle();
          check(readReducedMotionPreference() && localStorage.getItem('vrcs.reducedMotion') === 'true', 'The app preference is persisted');
          check(document.documentElement.dataset.reducedMotion === 'true', 'The preference applies to the document immediately');
          if (!${systemReduced}) check(!region.isConnected, 'Enabling reduced motion settles an in-progress collapse');
          if (master.getAttribute('aria-checked') === 'false') click(master);
          region = card.querySelector(':scope > .settings-disclosure');
          check(!region.getAnimations().length, 'App reduced motion skips disclosure animation');
          const profile = region.querySelector('.dropdown-trigger');
          click(profile);
          const menu = document.querySelector('.dropdown-menu');
          check(menu && !menu.getAnimations().length, 'App reduced motion skips dropdown animation');
          click(profile);
          check(!menu.isConnected, 'Reduced-motion dropdown dismissal is immediate');
          const details = card.querySelector('.vision-settings-details');
          getComputedStyle(details, '::details-content').blockSize;
          click(details.querySelector('summary'));
          check(getComputedStyle(details, '::details-content').transitionProperty === 'none', 'App reduced motion skips native details transitions');
          const beforeTheme = themeTransitions;
          updateThemePreference('dark');
          check(themeTransitions === beforeTheme, 'App reduced motion skips theme cross-fades');
          stopMotion();
          const restartMotion = initializeMotionPreference();
          check(readReducedMotionPreference() && document.documentElement.dataset.reducedMotion === 'true', 'Startup restores the saved motion preference');
          localStorage.setItem('vrcs.reducedMotion', 'false');
          flushSync(() => window.dispatchEvent(new StorageEvent('storage', { key: 'vrcs.reducedMotion' })));
          check(reduce.getAttribute('aria-checked') === 'false', 'Preference updates from other windows refresh the switch');
          check(document.documentElement.dataset.reducedMotion === String(${systemReduced}), 'Disabling the app override follows the system preference');
          click(master);
          region = card.querySelector(':scope > .settings-disclosure');
          if (!${systemReduced}) { check(region.getAnimations().length === 1, 'Disabling reduced motion restores animations'); await finish(region); }
          else check(!region, 'System reduced motion remains active when the app toggle is off');
          check(document.documentElement.scrollWidth <= innerWidth + 1, 'General settings and disclosures fit the viewport');
          restartMotion();
          root.unmount();
          document.getElementById('result').textContent = 'PASS';
        } catch (error) { document.getElementById('result').textContent = 'FAIL: ' + error.message; }
        parent.postMessage(document.getElementById('result').textContent, '*');
      ` },
      plugins: [{ name: 'locale-fixture', setup(build) {
        build.onResolve({ filter: /i18n\/catalog$/ }, () => ({ path: 'locale-catalog', namespace: 'fixture' }));
        build.onLoad({ filter: /.*/, namespace: 'fixture' }, () => ({
          resolveDir: join(testDirectory, '..'),
          contents: 'import en from "./src/i18n/locales/en-US.json"; export const localeCatalog = [en]; export const supportedUiLocales = ["en-US"]; export const localeResources = { "en-US": { translation: en.translation } };',
        }));
      } }],
      bundle: true, write: false, outfile: "settings-motion-test.js", format: "esm", platform: "browser", jsx: "automatic",
      define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
    });
    const directory = mkdtempSync(join(testDirectory, ".settings-motion-browser-"));
    try {
      const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
      const stylesheet = bundle.outputFiles.find(file => file.path.endsWith(".css")).text;
      const errorCapture = '<script>const reportError = message => parent.postMessage("FAIL: " + message, "*"); window.addEventListener("error", event => reportError(event.message)); window.addEventListener("unhandledrejection", event => reportError(String(event.reason)));</script>';
      writeFileSync(join(directory, "content.html"), '<style>' + stylesheet
        + '</style><div id="root" class="settings-surface"></div><pre id="result">PENDING</pre>' + errorCapture + '<script type="module">'
        + javascript.replace(/<\/script/gi, "<\\/script") + '</script>');
      const html = join(directory, "test.html");
      writeFileSync(html, '<body style="margin:0"><iframe src="content.html" style="border:0;width:' + width
        + 'px;height:900px"></iframe><pre id="result">PENDING</pre><script>window.addEventListener("message", event => document.getElementById("result").textContent = event.data);</script></body>');
      const browserProcess = spawn(browser, [
        "--headless=new", "--remote-debugging-port=0", "--disable-gpu", "--no-first-run",
        "--disable-background-networking", "--disable-extensions", "--window-size=" + Math.max(500, width) + ",900",
        ...(systemReduced ? ["--force-prefers-reduced-motion=reduce"] : []),
        "--user-data-dir=" + join(directory, "profile"), pathToFileURL(html).href,
      ], { timeout: 30000 });
      let socket;
      try {
        const endpoint = await new Promise((resolve, reject) => {
          let stderr = "";
          browserProcess.once("error", reject);
          browserProcess.once("exit", code => reject(new Error("Browser exited: " + code + " " + stderr)));
          browserProcess.stderr.on("data", chunk => {
            stderr += chunk;
            const match = stderr.match(/DevTools listening on (ws:\/\/[^\s]+)/);
            if (match) resolve(match[1]);
          });
        });
        socket = new WebSocket(endpoint);
        await new Promise((resolve, reject) => {
          socket.addEventListener("open", resolve, { once: true });
          socket.addEventListener("error", reject, { once: true });
        });
        let id = 0;
        const pending = new Map();
        socket.addEventListener("close", () => {
          for (const request of pending.values()) request.reject(new Error("Browser connection closed"));
          pending.clear();
        });
        socket.addEventListener("message", event => {
          const response = JSON.parse(event.data);
          const request = pending.get(response.id);
          if (request) {
            pending.delete(response.id);
            response.error ? request.reject(response.error) : request.resolve(response.result);
          }
        });
        const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
          const requestId = ++id;
          pending.set(requestId, { resolve, reject });
          socket.send(JSON.stringify({ id: requestId, method, params, sessionId }));
        });
        const { targetInfos } = await send("Target.getTargets");
        const page = targetInfos.find(target => target.type === "page");
        const { sessionId } = await send("Target.attachToTarget", { targetId: page.targetId, flatten: true });
        let report;
        for (let i = 0; i < 150; i++) {
          const response = await send("Runtime.evaluate", { expression: 'document.getElementById("result")?.textContent', returnByValue: true }, sessionId);
          report = response.result?.value;
          if (report && report !== "PENDING") break;
          await new Promise(resolve => setTimeout(resolve, 100));
        }
        assert.equal(report, "PASS");
      } finally {
        socket?.close();
        browserProcess.kill();
        await new Promise(resolve => browserProcess.exitCode !== null ? resolve() : browserProcess.once("exit", resolve));
      }
    } finally {
      rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
    }
  });
}
