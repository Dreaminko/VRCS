// Run with an installed Chrome or Edge: node --test tests/confirm-dialog.browser.mjs
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

const helpSource = `
  const check = (condition, message) => { if (!condition) throw new Error(message); };
  const tick = () => new Promise(resolve => setTimeout(resolve, 25));
  const wait = async (predicate, label) => {
    for (let attempt = 0; attempt < 200; attempt++) {
      if (predicate()) return;
      await tick();
    }
    throw new Error('Timed out waiting for ' + label);
  };
  const dialogByLabel = label => document.querySelector('[role="dialog"][aria-label="' + label + '"]');
  const actionsOf = dialog => [...dialog.querySelectorAll('.settings-inline-actions button')];
  const pressTab = () => document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));
  const pressEscape = () => document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
`;

async function runBrowserCheck({ contents, fixtureSource }) {
  const bundle = await build({
    stdin: {
      resolveDir: join(testDirectory, ".."),
      loader: "tsx",
      contents,
    },
    plugins: [{
      name: "confirm-dialog-fixture",
      setup(build) {
        build.onResolve({ filter: /^react-i18next$/ }, () => ({ path: "react-i18next", namespace: "fixture" }));
        build.onResolve({ filter: /storage\/api$/ }, () => ({ path: "storage-api", namespace: "fixture" }));
        build.onLoad({ filter: /.*/, namespace: "fixture" }, () => ({ contents: fixtureSource }));
      },
    }],
    bundle: true,
    write: false,
    outfile: "confirm-dialog-test.js",
    format: "esm",
    platform: "browser",
    jsx: "automatic",
    define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
  });
  const directory = mkdtempSync(join(tmpdir(), "vrcs-confirm-dialog-"));
  try {
    const content = join(directory, "content.html");
    const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
    const stylesheet = bundle.outputFiles.find(file => file.path.endsWith(".css"))?.text ?? "";
    const errorCapture = '<script>const result = document.getElementById("result"); const reportError = message => { result.textContent = "FAIL: " + message; }; window.addEventListener("error", event => reportError(event.message)); window.addEventListener("unhandledrejection", event => reportError(String(event.reason)));</script>';
    writeFileSync(content, '<style>' + stylesheet + '</style><div id="root" class="settings-surface"></div><pre id="result">PENDING</pre>'
      + errorCapture + '<script type="module">' + javascript.replace(/<\/script/gi, "<\\/script") + '</script>');
    const result = spawnSync(browser, [
      "--headless=new", "--dump-dom", "--virtual-time-budget=10000", "--disable-gpu", "--no-first-run",
      "--force-prefers-reduced-motion=reduce",
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
}

test("settings confirmations replace the native dialog and restore focus", {
  timeout: 60000, skip: !browser && "No installed Chrome or Edge executable was found.",
}, async () => {
  await runBrowserCheck({
    fixtureSource: `
      export const useTranslation = () => ({ t: key => key, i18n: { language: "en-US" } });
      const stats = { used_bytes: 2048, max_bytes: 104857600, allocated_bytes: 4096, reclaimable_bytes: 2048, over_limit: false };
      export const storageApi = {
        storageStats: async () => stats,
        clearSubtitleHistory: async () => {
          window.clears = (window.clears || 0) + 1;
          return { ...stats, used_bytes: 0, allocated_bytes: 0, reclaimable_bytes: 0 };
        },
      };
    `,
    contents: `
      import React from 'react';
      import { createRoot } from 'react-dom/client';
      import { StorageSettingsSection } from './src/settings/sections/StorageSettingsSection.tsx';
      import './src/styles.css';

      const nativeConfirms = [];
      window.confirm = message => { nativeConfirms.push(message); return true; };
      window.clears = 0;
      createRoot(document.getElementById('root')).render(<StorageSettingsSection
        locale="en-US"
        draft={{ storage: { subtitle_history_max_bytes: 104857600 } }}
        applySettings={() => {}} />);

      ${helpSource}
      try {
        await wait(() => document.querySelector('.storage-clear-button'), 'storage clear button');
        const trigger = document.querySelector('.storage-clear-button');
        check(trigger.checkVisibility(), 'storage controls are visible without expanding');
        const clearDialog = () => dialogByLabel('settings.storage.clearHistory');

        trigger.focus();
        trigger.click();
        await wait(clearDialog, 'in-app clear dialog');
        check(document.querySelectorAll('[role="dialog"]').length === 1, 'one in-app dialog is open');
        check(clearDialog().textContent.includes('settings.storage.clearConfirm'), 'dialog carries the confirmation message');
        check(nativeConfirms.length === 0, 'opening the confirmation must not use the native dialog');

        const cancel = () => actionsOf(clearDialog())[0];
        const confirmAction = () => actionsOf(clearDialog())[1];
        check(confirmAction().classList.contains('api-danger-button'), 'the confirm action keeps the danger styling');

        cancel().click();
        await wait(() => !clearDialog(), 'dialog closes on cancel');
        check(window.clears === 0, 'cancel must not clear the history');
        check(document.activeElement === trigger, 'focus returns to the trigger after cancel');

        trigger.focus();
        trigger.click();
        await wait(clearDialog, 'reopened clear dialog');
        pressEscape();
        await wait(() => !clearDialog(), 'dialog closes on Escape');
        check(window.clears === 0, 'Escape must not clear the history');

        trigger.focus();
        trigger.click();
        await wait(clearDialog, 'reopened clear dialog');
        confirmAction().click();
        await wait(() => window.clears === 1, 'clear request');
        await wait(() => !clearDialog(), 'dialog closes after clearing');
        check(nativeConfirms.length === 0, 'no native confirm was used for the whole flow');
        document.getElementById('result').textContent = 'PASS';
      } catch (error) {
        document.getElementById('result').textContent = 'FAIL: ' + error.message;
      }
    `,
  });
});

test("a confirmation inside a settings editor keeps its own focus and restores page scrolling", {
  timeout: 60000, skip: !browser && "No installed Chrome or Edge executable was found.",
}, async () => {
  await runBrowserCheck({
    fixtureSource: `
      export const useTranslation = () => ({ t: key => key, i18n: { language: "en-US" } });
    `,
    contents: `
      import React from 'react';
      import { createRoot } from 'react-dom/client';
      import { LocalGlossaryDialog } from './src/settings/glossary/GlossaryDialogs.tsx';
      import './src/styles.css';

      function Harness() {
        const [draft, setDraft] = React.useState({ id: null, name: '', entries: [] });
        const [closed, setClosed] = React.useState(false);
        if (closed) return <span id="closed">closed</span>;
        return <>
          <button id="make-dirty" type="button" onClick={() => setDraft(current => ({ ...current, name: 'changed' }))}>dirty</button>
          <LocalGlossaryDialog
            draft={draft}
            saving={false}
            error=""
            returnFocusRef={{ current: null }}
            onChange={setDraft}
            onError={() => {}}
            onSave={() => {}}
            onClose={() => setClosed(true)} />
        </>;
      }
      createRoot(document.getElementById('root')).render(<Harness />);

      ${helpSource}
      try {
        const editorDialog = () => dialogByLabel('settings.glossary.addGlossary');
        const discardDialog = () => dialogByLabel('settings.glossary.table.discardChanges');

        await wait(editorDialog, 'glossary editor dialog');
        document.getElementById('make-dirty').click();
        await tick();
        check(document.body.style.overflow === 'hidden', 'an open dialog hides page scrolling');

        document.querySelector('.api-profile-dialog-close').click();
        await wait(discardDialog, 'discard confirmation');
        check(Boolean(editorDialog()), 'the editor stays open behind the discard confirmation');
        check(document.body.style.overflow === 'hidden', 'the nested dialog keeps page scrolling hidden');

        const focused = document.activeElement;
        check(discardDialog().contains(focused), 'the confirmation owns the focus while it is on top');
        pressTab();
        check(document.activeElement === focused, 'the editor below the confirmation ignores its key events');

        actionsOf(discardDialog())[1].click();
        await wait(() => document.getElementById('closed'), 'editor close');
        await wait(() => !discardDialog() && !editorDialog(), 'both dialogs unmount');
        check(document.body.style.overflow === '', 'page scrolling is restored once every dialog is closed');
        document.getElementById('result').textContent = 'PASS';
      } catch (error) {
        document.getElementById('result').textContent = 'FAIL: ' + error.message;
      }
    `,
  });
});
