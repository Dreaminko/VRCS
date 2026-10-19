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

for (const width of [1200, 380]) {
  test(`conversation title settings, search, and actions work at ${width}px`, {
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
        import { ConversationTitleSettingsSection } from './src/settings/sections/ConversationTitleSettingsSection';
        import { ConversationSearchResults } from './src/conversations/ConversationSearchResults';
        import { ConversationActionsPopover } from './src/conversations/ConversationActionsPopover';
        import { conversationsFromCatalog } from './src/conversations/conversations';
        import './src/styles.css';
        await i18next.use(initReactI18next).init({ lng: 'en-US', resources: { 'en-US': { translation: en.translation } } });
        window.requestAnimationFrame = callback => window.setTimeout(() => callback(performance.now()), 16);
        window.cancelAnimationFrame = id => window.clearTimeout(id);
        window.fetch = async () => new Response(JSON.stringify({ models: ['title-model'], model_details: {} }));
        const t = key => i18next.t(key);
        const check = (condition, message) => { if (!condition) throw new Error(message); };
        const settle = () => new Promise(resolve => setTimeout(resolve, 30));
        const wait = async predicate => {
          for (let attempt = 0; attempt < 100; attempt++) {
            if (predicate()) return;
            await settle();
          }
          throw new Error('Timed out waiting for: ' + predicate.toString());
        };
        const noop = () => {};
        const profile = { id: 'titles', name: 'Title service', provider: 'openai_compatible',
          capabilities: { supports_text_generation: true } };
        let config;
        function Settings() {
          const [value, setValue] = useState({ auto_generate: false, profile_id: null, model: '', output_language: 'auto' });
          config = value;
          return <div className="settings-section settings-section-active system-section">
            <ConversationTitleSettingsSection config={value} profiles={[profile]}
              onChange={patch => setValue(current => ({ ...current, ...patch }))} onManageProfiles={noop} />
          </div>;
        }
        const root = createRoot(document.getElementById('root'));
        const render = element => flushSync(() => root.render(element));
        const buttonNamed = label => [...document.querySelectorAll('button[aria-label]')].find(button => button.getAttribute('aria-label') === label);
        const choose = async (label, text) => {
          buttonNamed(label).click();
          await wait(() => [...document.querySelectorAll('[role=option]')].some(option => option.textContent.trim() === text));
          [...document.querySelectorAll('[role=option]')].find(option => option.textContent.trim() === text).click();
          await settle();
        };
        const conversation = conversationsFromCatalog({ conversations: [{
          id: 'past', started_at: '2026-10-19T10:00:00Z', ended_at: '2026-10-19T11:00:00Z',
          automatic_title: 'Hello', summary_title: 'Japanese interview practice', custom_title: null,
          icon: null, subtitle_count: 10, updated_at: '2026-10-19T11:00:00Z', active: false,
        }] })[0];
        let titleSelections = 0;
        let subtitleSelections = 0;
        let generations = 0;
        let settingsVisits = 0;
        const search = state => <ConversationSearchResults query="INTERVIEW" items={[]} conversations={[conversation]}
          loading={false} loadingMore={false} hasMore={false} failed={false} searchable={true} {...state}
          onLoadMore={async () => {}} onSelect={() => subtitleSelections++}
          onSelectConversation={id => { check(id === 'past', 'Title hit selects its own conversation'); titleSelections++; }} />;
        const actions = (patch = {}, configured = true) => <ConversationActionsPopover conversation={{ ...conversation, ...patch }}
          position={{ top: 20, left: 20 }} choosingIcon={false} titleConfigured={configured}
          onChooseIcon={noop} onStartRename={noop} onShowIconPicker={noop} onResetCustomization={noop}
          onGenerateTitle={() => generations++} onOpenTitleSettings={() => settingsVisits++} onDelete={noop} onClose={noop} />;
        const menuButton = key => [...document.querySelectorAll('[role=menuitem]')].find(button => button.textContent === t(key));
        try {
          render(<Settings />);
          await settle();
          const toggle = () => document.querySelector('[role=switch]');
          check(toggle().disabled, 'Automatic titles require a profile and model');
          check(document.querySelector('details').open, 'Unconfigured model settings are discoverable');
          await choose(t('settings.conversationTitles.profile'), 'Title service');
          check(toggle().disabled, 'A profile alone does not enable automatic titles');
          await wait(() => !buttonNamed(t('settings.conversationTitles.model')).disabled);
          await choose(t('settings.conversationTitles.model'), 'title-model');
          check(!toggle().disabled, 'A configured model enables the automatic title switch');
          check(document.querySelector('details').open, 'Choosing a model does not collapse its settings');
          toggle().click();
          await settle();
          check(config.auto_generate, 'The switch enables automatic generation');
          toggle().click();
          await settle();
          check(!config.auto_generate && config.profile_id === 'titles' && config.model === 'title-model',
            'Disabling automatic titles preserves manual generation configuration');
          document.querySelector('summary').click();
          await settle();
          check(!document.querySelector('details').open, 'Model settings can be collapsed');
          document.querySelector('summary').click();
          await settle();
          const input = document.querySelector('input[role=combobox]');
          const refresh = buttonNamed(t('settings.apiManagement.refreshModels'));
          const inputRect = input.getBoundingClientRect();
          const refreshRect = refresh.getBoundingClientRect();
          check(Math.abs(inputRect.top - refreshRect.top) <= 1 && inputRect.right <= refreshRect.left,
            'The model input and refresh button align without overlap');
          check(innerWidth === ${width}, 'The test uses the expected CSS viewport');
          check(document.documentElement.scrollWidth <= innerWidth + 1, 'Conversation settings do not overflow horizontally');

          for (const state of [{ loading: true }, { failed: true }, { searchable: false }]) {
            render(search(state));
            const hit = document.querySelector('.conversation-search-result');
            check(hit?.textContent === conversation.title, 'Title hits remain visible independently of subtitle search state');
            hit.click();
          }
          check(titleSelections === 3 && subtitleSelections === 0, 'Title hits open the conversation without a subtitle target');
          render(search({}));
          check(![...document.querySelectorAll('.conversation-search-status')].some(status => status.textContent.includes(t('conversations.searchEmpty'))),
            'A title hit suppresses the empty result message');
          for (const titleStatus of ['pending', 'running']) {
            render(actions({ titleStatus }));
            check(menuButton('conversations.titleGenerating')?.disabled, 'Queued and running requests cannot be duplicated');
          }
          render(actions({ titleStatus: 'failed', titleError: 'conversations.title_timeout' }));
          check(document.querySelector('.conversation-action-note')?.textContent === t('errors.conversations.title_timeout'),
            'Failed title generation shows a readable localized reason');
          menuButton('conversations.titleRegenerate').click();
          check(generations === 1, 'A failed generation can be retried');
          render(actions({ hasCustomTitle: true, customized: true }));
          check(menuButton('conversations.titleRegenerate')?.disabled, 'Manual titles block summary generation');
          render(actions({}, false));
          menuButton('conversations.titleRegenerate').click();
          check(settingsVisits === 1 && generations === 1, 'An unconfigured model routes the action to settings');
          render(actions({ active: true }));
          check(!menuButton('conversations.titleRegenerate'), 'An active conversation has no generate action');
          render(actions({ subtitleCount: 0 }));
          check(!menuButton('conversations.titleRegenerate'), 'An empty conversation has no generate action');
          root.unmount();
          document.getElementById('result').textContent = 'PASS';
        } catch (error) { document.getElementById('result').textContent = 'FAIL: ' + error.message; }
      ` },
      plugins: [{ name: 'locale-catalog-fixture', setup(build) {
        build.onResolve({ filter: /i18n\/catalog$/ }, () => ({ path: 'locale-catalog', namespace: 'fixture' }));
        build.onLoad({ filter: /.*/, namespace: 'fixture' }, () => ({
          resolveDir: join(testDirectory, '..'),
          contents: 'import en from "./src/i18n/locales/en-US.json"; export const localeCatalog = [en]; export const supportedUiLocales = ["en-US"]; export const localeResources = { "en-US": { translation: en.translation } };',
        }));
      } }],
      bundle: true, write: false, outfile: 'conversation-titles-test.js', format: 'esm', platform: 'browser', jsx: 'automatic',
      define: { 'import.meta.env.VITE_VRCS_SESSION_TOKEN': '""' },
    });
    const directory = mkdtempSync(join(testDirectory, '.conversation-titles-browser-'));
    try {
      const html = join(directory, 'test.html');
      const javascript = bundle.outputFiles.find(file => file.path.endsWith('.js')).text;
      const stylesheet = bundle.outputFiles.find(file => file.path.endsWith('.css')).text;
      writeFileSync(join(directory, 'content.html'), '<style>' + stylesheet + '</style><div id="root" class="settings-surface"></div><pre id="result">PENDING</pre>'
        + '<script>const result = document.getElementById("result"); window.addEventListener("error", event => result.textContent = "FAIL: " + event.message); window.addEventListener("unhandledrejection", event => result.textContent = "FAIL: " + event.reason); new MutationObserver(() => parent.postMessage(result.textContent, "*")).observe(result, { childList: true });</script>'
        + '<script type="module">' + javascript.replace(/<\/script/gi, '<\\/script') + '</script>');
      // The iframe avoids the browser window's minimum viewport width.
      writeFileSync(html, '<iframe src="content.html" style="display:block;border:0;width:' + width + 'px;height:900px"></iframe><pre id="result">PENDING</pre>'
        + '<script>window.addEventListener("message", event => { if (event.source === document.querySelector("iframe").contentWindow) document.getElementById("result").textContent = event.data; });</script>');
      const result = spawnSync(browser, [
        '--headless=new', '--dump-dom', '--virtual-time-budget=10000', '--disable-gpu', '--no-first-run',
        '--disable-background-networking', '--disable-extensions', '--window-size=' + width + ',900',
        '--user-data-dir=' + join(directory, 'profile'), pathToFileURL(html).href,
      ], { encoding: 'utf8', timeout: 30000, maxBuffer: 8 * 1024 * 1024 });
      assert.ifError(result.error);
      assert.equal(result.status, 0, result.stderr);
      const report = result.stdout.match(/<pre id="result">([^<]*)<\/pre>/)?.[1];
      assert.equal(report, 'PASS', report || result.stderr);
    } finally {
      rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
    }
  });
}
