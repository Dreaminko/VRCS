// Run with an installed Chrome or Edge: node --test tests/provider-icon.browser.mjs
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
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

test("api management rows render lobe brand marks for light and dark themes", {
  timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
}, async () => {
  const bundle = await build({
    stdin: {
      resolveDir: join(testDirectory, ".."),
      loader: "tsx",
      contents: `
        import React from 'react';
        import { createRoot } from 'react-dom/client';
        import { ApiManagementSettingsSection } from './src/settings/sections/ApiManagementSettingsSection.tsx';
        import './src/styles.css';

        const capabilities = {
          supports_streaming: false, supports_model_listing: false, requires_api_key: true,
          is_local: false, supports_context: false, supports_translation: false,
          supports_asr: false, supports_text_generation: true, supports_image_understanding: false,
          supports_custom_translation_language: false, supported_languages: [],
        };
        const profile = (id, name, provider, providerName) => ({
          id, name, provider, provider_display_name: providerName,
          enabled_capabilities: ['text_generation'], active: false, translation_active: false,
          credential: { configured: false, stored_configured: false, environment_override: false, storage_available: true, reason_code: null, source: null },
          capabilities, support_levels: { asr: null, translation: null },
        });
        const definition = (id, displayName) => ({
          id, display_name: displayName, category: 'cloud_provider',
          connection: { base_url: { mode: 'fixed', default: null }, auth_modes: ['bearer'], default_auth_mode: 'bearer', fields: [] },
          services: [], support_levels: { asr: null, translation: null }, capabilities,
        });
        window.fixture = {
          profiles: [
            profile('openai-profile', 'OpenAI profile', 'openai', 'OpenAI'),
            profile('qwen-profile', 'Qwen profile', 'qwen_ai', 'Qwen AI'),
            profile('deepl-profile', 'DeepL profile', 'deepl', 'DeepL'),
            profile('custom-profile', 'Custom profile', 'openai_compatible', 'Custom OpenAI Compatible'),
          ],
          definitions: [
            definition('openai', 'OpenAI'),
            definition('qwen_ai', 'Qwen AI'),
            definition('deepl', 'DeepL'),
            definition('openai_compatible', 'Custom OpenAI Compatible'),
          ],
        };
        createRoot(document.getElementById('root')).render(<ApiManagementSettingsSection
          settings={{ asr: { backend: '', language: 'auto', managed_qwen: { package_id: '', device: 'auto' },
            active_profile_id: null, service_settings: {} }, translation: { mode: 'disabled', speaker_targets: [] } }}
          onRefreshSettings={async () => {}} />);

        const check = (condition, message) => { if (!condition) throw new Error(message); };
        const tick = () => new Promise(resolve => setTimeout(resolve, 25));
        const wait = async (predicate, label) => {
          for (let attempt = 0; attempt < 200; attempt++) {
            if (predicate()) return;
            await tick();
          }
          throw new Error('Timed out waiting for ' + label);
        };
        const iconOf = label => document.querySelector('[aria-label="' + label + '"]').querySelector('.api-profile-icon');
        const markOf = label => iconOf(label).querySelector('svg');
        try {
          await wait(() => document.querySelectorAll('.api-profile-row').length === 4, 'provider rows');

          for (const label of ['OpenAI profile', 'Qwen profile', 'DeepL profile']) {
            check(!iconOf(label).querySelector('img'), label + ' must not render an img placeholder');
            check(markOf(label), label + ' renders a brand mark');
            const rect = markOf(label).getBoundingClientRect();
            check(Math.round(rect.width) === 18 && Math.round(rect.height) === 18, label + ' keeps the 18px row size');
          }
          const customIcon = iconOf('Custom profile');
          check(!customIcon.querySelector('svg'), 'custom provider has no brand mark');
          check(customIcon.querySelector('strong')?.textContent === 'C', 'custom provider uses the initial fallback');

          check(markOf('OpenAI profile').getAttribute('fill') === 'currentColor', 'OpenAI has no color variant and follows the theme ink');
          check(markOf('Qwen profile').innerHTML.includes('#6336E7'), 'Qwen mark carries the brand gradient');
          check(markOf('DeepL profile').innerHTML.includes('#0F2B46'), 'DeepL mark carries the brand color');

          document.documentElement.setAttribute('data-theme', 'light');
          check(getComputedStyle(iconOf('OpenAI profile')).backgroundColor === 'rgb(245, 248, 251)', 'light theme keeps the canvas plate');
          check(getComputedStyle(markOf('OpenAI profile')).fill === getComputedStyle(iconOf('OpenAI profile')).color,
            'OpenAI mark follows the light theme ink');
          const deeplFill = () => getComputedStyle(markOf('DeepL profile').querySelector('path')).fill;
          check(deeplFill() === 'rgb(15, 43, 70)', 'DeepL keeps its brand color in light theme');

          document.documentElement.setAttribute('data-theme', 'dark');
          check(getComputedStyle(iconOf('OpenAI profile')).backgroundColor === 'rgb(228, 233, 240)',
            'dark theme keeps a light plate so brand colors stay legible');
          check(getComputedStyle(markOf('OpenAI profile')).fill === 'rgb(29, 40, 52)', 'OpenAI mark uses dark ink on the light plate');
          check(deeplFill() === 'rgb(15, 43, 70)', 'DeepL keeps its brand color in dark theme');
          check(markOf('Qwen profile').querySelector('path').getAttribute('fill').startsWith('url('),
            'Qwen mark keeps painting its brand gradient in dark theme');
          document.getElementById('result').textContent = 'PASS';
        } catch (error) {
          document.getElementById('result').textContent = 'FAIL: ' + error.message;
        }
      `,
    },
    plugins: [{
      name: "providers-api-fixture",
      setup(build) {
        build.onResolve({ filter: /^react-i18next$/ }, () => ({ path: "react-i18next", namespace: "fixture" }));
        build.onResolve({ filter: /providers\/api$/ }, () => ({ path: "providers-api", namespace: "fixture" }));
        build.onLoad({ filter: /.*/, namespace: "fixture" }, () => ({
          contents: `
            export const useTranslation = () => ({ t: key => key });
            export const providersApi = {
              apiProfiles: async () => ({ profiles: window.fixture.profiles }),
              providers: async () => ({ providers: window.fixture.definitions }),
              apiProfileModels: async () => ({ models: [] }),
            };
          `,
        }));
      },
    }],
    bundle: true,
    write: false,
    outfile: "provider-icon-test.js",
    format: "esm",
    platform: "browser",
    jsx: "automatic",
    define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
  });
  const directory = mkdtempSync(join(tmpdir(), "vrcs-provider-icon-browser-"));
  try {
    const content = join(directory, "content.html");
    const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
    const stylesheet = bundle.outputFiles.find(file => file.path.endsWith(".css"))?.text ?? "";
    const errorCapture = '<script>const result = document.getElementById("result"); const reportError = message => { result.textContent = "FAIL: " + message; }; window.addEventListener("error", event => reportError(event.message)); window.addEventListener("unhandledrejection", event => reportError(String(event.reason)));</script>';
    writeFileSync(content, '<style>' + stylesheet + '</style><div id="root" class="settings-surface"></div><pre id="result">PENDING</pre>'
      + errorCapture + '<script type="module">' + javascript.replace(/<\/script/gi, "<\\/script") + '</script>');
    const result = spawnSync(browser, [
      "--headless=new", "--dump-dom", "--virtual-time-budget=10000", "--disable-gpu", "--no-first-run",
      "--disable-background-networking", "--disable-extensions", "--disable-software-rasterizer",
      "--no-sandbox", "--enable-logging=stderr", "--window-size=1200,900",
      "--user-data-dir=" + join(directory, "profile"), pathToFileURL(content).href,
    ], { encoding: "utf8", timeout: 30000, maxBuffer: 8 * 1024 * 1024 });
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    const report = result.stdout.match(/<pre id="result">([^<]*)<\/pre>/)?.[1];
    assert.equal(report, "PASS", `${report || ""}\n${result.stderr}\n${result.stdout.slice(-3000)}`);
  } finally {
    rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
});
