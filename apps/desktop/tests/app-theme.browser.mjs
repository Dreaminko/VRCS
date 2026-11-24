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

test("dark theme applies tokens through the document root and settings surfaces", {
  timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
}, async () => {
  const bundle = await build({
    stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
      import React from 'react';
      import { createRoot } from 'react-dom/client';
      import { useAppTheme } from './src/app/useAppTheme';
      import './src/styles.css';

      function ThemeProbe() {
        const { themePreference, setThemePreference } = useAppTheme();
        return (
          <div className="settings-surface">
            <nav className="settings-tabbar">
              <button id="to-light" onClick={() => setThemePreference('light')}>{themePreference}</button>
              <button id="to-dark" className="active" onClick={() => setThemePreference('dark')}>{themePreference}</button>
            </nav>
            <div className="theme-toggle-group" role="group">
              <button id="segment-dark" className="active" onClick={() => setThemePreference('dark')}>dark</button>
            </div>
          </div>
        );
      }
      createRoot(document.getElementById('root')).render(<ThemeProbe />);
      // Headless Chrome's virtual clock does not advance compositor-driven
      // transitions or view transitions, so pin both to their end state.
      Object.defineProperty(document, 'startViewTransition', { value: undefined, configurable: true });
      const freeze = document.createElement('style');
      freeze.textContent = '* { transition: none !important; }';
      document.head.appendChild(freeze);
      const check = (condition, message) => { if (!condition) throw new Error(message); };
      const settle = () => new Promise(resolve => setTimeout(resolve, 20));
      const wait = async predicate => {
        for (let attempt = 0; attempt < 200; attempt++) {
          const value = predicate();
          if (value) return value;
          await settle();
        }
        throw new Error('Timed out waiting for: ' + predicate.toString());
      };
      const style = element => getComputedStyle(element);
      try {
        const root = document.documentElement;
        const lightButton = await wait(() => document.querySelector('#to-light'));
        const darkButton = document.querySelector('#to-dark');
        lightButton.click();
        await wait(() => root.getAttribute('data-theme') === 'light');
        await settle();
        check(localStorage.getItem('vrcs.theme') === 'light', 'light preference is persisted');
        check(style(root).colorScheme.includes('light'), 'light color scheme applies');
        check(style(root).getPropertyValue('--canvas').trim() === '#f5f8fb', 'light canvas token resolves');
        check(style(document.body).backgroundColor === 'rgb(245, 248, 251)', 'body paints the light canvas');

        darkButton.click();
        await wait(() => root.getAttribute('data-theme') === 'dark');
        await wait(() => style(darkButton).backgroundColor === 'rgb(69, 74, 86)');
        await wait(() => style(darkButton).color === 'rgb(127, 216, 255)');
        await settle();
        check(localStorage.getItem('vrcs.theme') === 'dark', 'dark preference is persisted');
        check(style(root).colorScheme.includes('dark'), 'dark color scheme applies');
        check(style(root).getPropertyValue('--canvas').trim() === '#282c33', 'dark canvas token resolves');
        check(style(root).getPropertyValue('--text').trim() === '#dce0e5', 'dark text token resolves');
        check(style(document.body).backgroundColor === 'rgb(40, 44, 51)', 'body paints the dark canvas');
        check(style(darkButton).getPropertyValue('--surface-hover').trim().startsWith('rgba(255, 255, 255, 0.07'), 'dark hover overlay token resolves');
        check(style(root).getPropertyValue('--primary').trim() === '#3fb9ec', 'dark primary token is toned down');
        check(style(root).getPropertyValue('--primary-ink').trim() === '#7fd8ff', 'dark accent ink is light');
        check(style(darkButton).backgroundColor === 'rgb(69, 74, 86)', 'active settings tab uses the dark selected surface');
        check(style(darkButton).color === 'rgb(127, 216, 255)', 'active settings tab uses the light accent ink');
        const segment = document.querySelector('#segment-dark');
        check(style(segment).backgroundColor === 'rgb(69, 74, 86)', 'active theme segment uses the dark selected surface');
        check(style(segment).color === 'rgb(127, 216, 255)', 'active theme segment uses the light accent ink');
        document.getElementById('result').textContent = 'PASS';
      } catch (error) { document.getElementById('result').textContent = 'FAIL: ' + error.message; }
    ` },
    bundle: true, write: false, outfile: 'app-theme-browser-test.js', format: 'esm', platform: 'browser', jsx: 'automatic',
    define: { 'import.meta.env.VITE_VRCS_SESSION_TOKEN': '""' },
  });

  const directory = mkdtempSync(join(testDirectory, '.app-theme-browser-'));
  try {
    const html = join(directory, 'test.html');
    const content = join(directory, 'content.html');
    const javascript = bundle.outputFiles.find(file => file.path.endsWith('.js')).text;
    const stylesheet = bundle.outputFiles.find(file => file.path.endsWith('.css')).text;
    const errorCapture = '<script>const result = document.getElementById("result"); const reportError = message => { result.textContent = "FAIL: " + message; }; window.addEventListener("error", event => reportError(event.message)); window.addEventListener("unhandledrejection", event => reportError(String(event.reason))); new MutationObserver(() => parent.postMessage(result.textContent, "*")).observe(result, { childList: true });</script>';
    writeFileSync(content, '<style>' + stylesheet + '</style><div id="root"></div><pre id="result">PENDING</pre>'
      + errorCapture + '<script type="module">' + javascript.replace(/<\/script/gi, '<\\/script') + '</script>');
    writeFileSync(html, '<iframe src="content.html" style="display:block;border:0;width:1200px;height:900px"></iframe><pre id="result">PENDING</pre>'
      + '<script>window.addEventListener("message", event => { if (event.source === document.querySelector("iframe").contentWindow) document.getElementById("result").textContent = event.data; });</script>');
    const result = spawnSync(browser, [
      '--headless=new', '--dump-dom', '--virtual-time-budget=10000', '--disable-gpu', '--no-first-run',
      '--disable-background-networking', '--disable-extensions', '--window-size=1200,900',
      '--user-data-dir=' + join(directory, 'profile'), pathToFileURL(html).href,
    ], { encoding: 'utf8', timeout: 30000, maxBuffer: 8 * 1024 * 1024 });
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, />PASS</, 'dark theme probe failed: ' + result.stdout.match(/<pre id="result">[^<]*/)?.[0]);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

for (const width of [1200, 380]) {
  test(`onboarding welcome step defaults to Follow system theme and switches themes at ${width}px`, {
    timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
  }, async () => {
    const bundle = await build({
      stdin: {
        resolveDir: join(testDirectory, ".."),
        loader: "tsx",
        contents: `
          import React from 'react';
          import { createRoot } from 'react-dom/client';
          import { flushSync } from 'react-dom';
          import i18next from 'i18next';
          import { I18nextProvider, initReactI18next } from 'react-i18next';
          import en from './src/i18n/locales/en-US.json';
          import { WelcomeStep } from './src/onboarding/steps/WelcomeStep';
          import './src/styles.css';

          document.documentElement.dataset.reducedMotion = 'true';
          Object.defineProperty(document, 'startViewTransition', { value: undefined, configurable: true });
          localStorage.clear();

          const freeze = document.createElement('style');
          freeze.textContent = '* { transition: none !important; }';
          document.head.appendChild(freeze);

          const i18n = i18next.createInstance();
          await i18n.use(initReactI18next).init({
            lng: 'en-US',
            resources: { 'en-US': { translation: en.translation } },
            interpolation: { escapeValue: false },
          });

          function Harness({ busy = false }) {
            return (
              <I18nextProvider i18n={i18n}>
                <div className="onboarding-scroll-region" style={{ height: '100%' }}>
                  <section className="onboarding-surface">
                    <aside className="onboarding-sidebar" />
                    <div className="onboarding-main">
                      <div className="onboarding-content">
                        <WelcomeStep
                          languagePreference="system"
                          busy={busy}
                          onUpdateLanguage={() => {}}
                          downloadSource="auto"
                          onUpdateDownloadSource={() => {}}
                        />
                      </div>
                    </div>
                  </section>
                </div>
              </I18nextProvider>
            );
          }

          const rootElement = document.getElementById('root');
          const reactRoot = createRoot(rootElement);
          const check = (condition, message) => { if (!condition) throw new Error(message); };
          const settle = () => new Promise((resolve) => setTimeout(resolve, 20));
          const wait = async (predicate, label) => {
            for (let attempt = 0; attempt < 200; attempt += 1) {
              const value = predicate();
              if (value) return value;
              await settle();
            }
            throw new Error('Timed out waiting for: ' + (label || predicate.toString()));
          };
          const click = (element) => { check(element, 'Missing element for click'); flushSync(() => element.click()); };
          const style = (element) => getComputedStyle(element);

          let stage = 'initial mount';
          try {
            flushSync(() => reactRoot.render(<Harness />));
            await settle();

            const root = document.documentElement;
            const languageSetting = await wait(
              () => document.querySelector('.onboarding-language-setting:not(.onboarding-theme-setting):not(.onboarding-download-setting)'),
              'language setting',
            );
            const themeSetting = document.querySelector('.onboarding-theme-setting');
            const downloadSetting = document.querySelector('.onboarding-download-setting');
            check(Boolean(languageSetting && themeSetting && downloadSetting), 'onboarding settings cards are rendered');
            check(Boolean(languageSetting.compareDocumentPosition(themeSetting) & Node.DOCUMENT_POSITION_FOLLOWING), 'theme setting appears below language setting');
            check(Boolean(themeSetting.compareDocumentPosition(downloadSetting) & Node.DOCUMENT_POSITION_FOLLOWING), 'download source appears below theme setting');

            const buttons = [...themeSetting.querySelectorAll('.theme-toggle-group button')];
            check(buttons.length === 3, 'renders Follow system, Light, and Dark buttons');
            const byLabel = (label) => buttons.find((button) => button.textContent?.trim() === label);
            const followSystemButton = byLabel('Follow system');
            const lightButton = byLabel('Light');
            const darkButton = byLabel('Dark');
            check(Boolean(followSystemButton && lightButton && darkButton), 'all three theme option labels are rendered');

            const expectedSystemTheme = window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
            check(followSystemButton.getAttribute('aria-pressed') === 'true' && followSystemButton.classList.contains('active'), 'Follow system is active by default');
            check(lightButton.getAttribute('aria-pressed') === 'false' && !lightButton.classList.contains('active'), 'Light is inactive by default');
            check(darkButton.getAttribute('aria-pressed') === 'false' && !darkButton.classList.contains('active'), 'Dark is inactive by default');
            check(localStorage.getItem('vrcs.theme') === null, 'mounting welcome step does not overwrite stored theme preference');
            check(root.getAttribute('data-theme') === expectedSystemTheme, 'initial theme matches system preference');
            check(root.scrollWidth <= innerWidth, 'onboarding welcome step fits viewport without horizontal overflow: ' + root.scrollWidth + ' > ' + innerWidth);

            const toggleGroup = themeSetting.querySelector('.theme-toggle-group');
            const groupBounds = toggleGroup.getBoundingClientRect();
            check(groupBounds.width > 0 && groupBounds.left >= 0 && groupBounds.right <= innerWidth, 'theme toggle group fits inside viewport');
            for (const button of buttons) {
              const bounds = button.getBoundingClientRect();
              check(bounds.width > 0 && bounds.left >= 0 && bounds.right <= innerWidth, 'theme button fits viewport: ' + button.textContent);
            }

            stage = 'select Dark';
            click(darkButton);
            await wait(() => darkButton.getAttribute('aria-pressed') === 'true' && root.getAttribute('data-theme') === 'dark', 'dark theme applied');
            await settle();
            check(darkButton.classList.contains('active'), 'Dark button is marked active');
            check(followSystemButton.getAttribute('aria-pressed') === 'false', 'Follow system is deselected after selecting Dark');
            check(localStorage.getItem('vrcs.theme') === 'dark', 'dark theme preference is persisted');
            check(style(root).getPropertyValue('--canvas').trim() === '#282c33', 'dark canvas token resolves');
            check(style(document.body).backgroundColor === 'rgb(40, 44, 51)', 'body paints dark canvas');

            stage = 'select Light';
            click(lightButton);
            await wait(() => lightButton.getAttribute('aria-pressed') === 'true' && root.getAttribute('data-theme') === 'light', 'light theme applied');
            await settle();
            check(lightButton.classList.contains('active'), 'Light button is marked active');
            check(darkButton.getAttribute('aria-pressed') === 'false', 'Dark is deselected after selecting Light');
            check(localStorage.getItem('vrcs.theme') === 'light', 'light theme preference is persisted');
            check(style(root).getPropertyValue('--canvas').trim() === '#f5f8fb', 'light canvas token resolves');
            check(style(document.body).backgroundColor === 'rgb(245, 248, 251)', 'body paints light canvas');

            stage = 'select Follow system';
            click(followSystemButton);
            await wait(
              () => followSystemButton.getAttribute('aria-pressed') === 'true' && root.getAttribute('data-theme') === expectedSystemTheme,
              'system theme restored',
            );
            await settle();
            check(followSystemButton.classList.contains('active'), 'Follow system button is marked active');
            check(lightButton.getAttribute('aria-pressed') === 'false', 'Light is deselected after selecting Follow system');
            check(localStorage.getItem('vrcs.theme') === 'system', 'system theme preference is persisted');

            stage = 'busy state';
            flushSync(() => reactRoot.render(<Harness busy={true} />));
            await settle();
            for (const button of themeSetting.querySelectorAll('.theme-toggle-group button')) {
              check(button.disabled === true, 'theme button disables when onboarding is busy: ' + button.textContent);
            }

            document.getElementById('result').textContent = 'PASS';
          } catch (error) {
            document.getElementById('result').textContent = 'FAIL (' + stage + '): ' + (error?.stack || error?.message || String(error));
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
      define: { "import.meta.env": "{}" },
      bundle: true,
      write: false,
      outfile: "onboarding-theme-test.js",
      format: "esm",
      platform: "browser",
      jsx: "automatic",
    });

    const directory = mkdtempSync(join(testDirectory, `.onboarding-theme-browser-${width}-`));
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
        `<body style="margin:0"><iframe src="content.html" style="display:block;border:0;width:${width}px;height:900px"></iframe><pre id="result">PENDING</pre><script>window.addEventListener("message", event => { if (event.source === document.querySelector("iframe").contentWindow) document.getElementById("result").textContent = event.data; });</script></body>`,
      );
      const result = spawnSync(browser, [
        "--headless=new",
        "--dump-dom",
        "--virtual-time-budget=15000",
        "--disable-gpu",
        "--no-first-run",
        "--disable-background-networking",
        "--disable-extensions",
        "--window-size=" + Math.max(500, width) + ",1000",
        "--user-data-dir=" + join(directory, "profile"),
        pathToFileURL(html).href,
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
