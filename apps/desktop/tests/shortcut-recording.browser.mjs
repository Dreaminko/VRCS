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

test("shortcut recording saves released chords, cancels safely and serializes native suspension", {
  timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
}, async () => {
  const bundle = await build({
    stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
      import React, { useState } from 'react';
      import { createRoot } from 'react-dom/client';
      import i18next from 'i18next';
      import { initReactI18next } from 'react-i18next';
      import en from './src/i18n/locales/en-US.json';
      import { ShortcutField } from './src/shared/ui/ShortcutField';
      import './src/styles.css';
      await i18next.use(initReactI18next).init({ lng: 'en-US', resources: { 'en-US': { translation: en.translation } } });
      const calls = [], updates = [], errors = [];
      window.addEventListener('error', event => errors.push(event.message));
      let delayStart = false, failStart = false, releaseStart;
      window.nativeInvoke = async (command, args) => {
        if (command !== 'set_shortcut_recording') throw new Error('Unexpected command: ' + command);
        calls.push(args.recording);
        if (args.recording && failStart) throw new Error('Suspension failed');
        if (args.recording && delayStart) await new Promise(resolve => { releaseStart = resolve; });
      };
      function Harness() {
        const [value, setValue] = useState('Ctrl+Alt+V');
        const [other, setOther] = useState('Ctrl+Alt+O');
        const [disabled, setDisabled] = useState(false);
        window.changeValue = setValue;
        window.disable = setDisabled;
        return <>
          <ShortcutField label="Vision shortcut" value={value} disabled={disabled}
            onChange={next => { updates.push(next); setValue(next); }} />
          <ShortcutField label="OCR shortcut" value={other}
            onChange={next => { updates.push(next); setOther(next); }} />
          <button id="outside">Outside</button>
        </>;
      }
      const root = createRoot(document.getElementById('root'));
      root.render(<React.StrictMode><Harness /></React.StrictMode>);
      const check = (condition, message) => { if (!condition) throw new Error(message); };
      const wait = async predicate => {
        for (let attempt = 0; attempt < 200; attempt++) {
          if (predicate()) return;
          await new Promise(resolve => setTimeout(resolve, 10));
        }
        throw new Error('Timed out waiting for the frontend: ' + predicate.toString());
      };
      const settle = () => new Promise(resolve => setTimeout(resolve, 20));
      const button = index => document.querySelectorAll('.shortcut-recorder')[index];
      const start = async (index = 0) => {
        button(index).focus(); button(index).click();
        await wait(() => button(index).textContent.includes('Press a shortcut'));
      };
      const event = (type, key, code, modifiers = {}) => {
        const input = new KeyboardEvent(type, { key, code, bubbles: true, cancelable: true, ...modifiers });
        button(0).dispatchEvent(input);
        return input;
      };
      const chord = async (key = 'k', code = 'KeyK') => {
        event('keydown', 'Control', 'ControlLeft', { ctrlKey: true });
        event('keydown', 'Shift', 'ShiftLeft', { ctrlKey: true, shiftKey: true });
        check(event('keydown', key, code, { ctrlKey: true, shiftKey: true }).defaultPrevented, 'Recorded key prevents browser shortcuts');
        event('keydown', key, code, { ctrlKey: true, shiftKey: true, repeat: true });
        await settle();
        check(updates.length === 0, 'A held chord is not saved yet');
        check(button(0).textContent.includes(key.toUpperCase()), 'Recorded chord is previewed');
        event('keyup', key, code, { ctrlKey: true, shiftKey: true });
        event('keyup', 'Shift', 'ShiftLeft', { ctrlKey: true });
        await settle();
        check(updates.length === 0, 'Registration waits for remaining modifiers');
        event('keyup', 'Control', 'ControlLeft');
      };
      try {
        await wait(() => button(0));
        await settle();
        check(!document.querySelector('input'), 'Shortcut preview is not a text input');
        check(!document.querySelector('.shortcut-field small, .shortcut-recorder-action'), 'Idle fields have no helper copy or change prompt');
        const labelRect = document.querySelector('.shortcut-field > span').getBoundingClientRect();
        const buttonRect = button(0).getBoundingClientRect();
        check(labelRect.right <= buttonRect.left && Math.abs((labelRect.top + labelRect.bottom - buttonRect.top - buttonRect.bottom) / 2) <= 1,
          'Shortcut label and preview button share a centered row');
        await start();
        check(calls.join(',') === 'true', 'Native suspension starts before listening');
        await chord();
        await wait(() => updates.length === 1 && calls.at(-1) === false);
        check(updates[0] === 'Ctrl+Shift+K', 'Chord is normalized and saved once');
        check(button(0).getAttribute('aria-pressed') === 'false', 'Recorder exits after release');
        check(button(0).getBoundingClientRect().right <= innerWidth, 'Shortcut preview fits a narrow window');
        updates.length = 0;
        await start();
        event('keydown', 'Escape', 'Escape');
        await wait(() => calls.at(-1) === false);
        check(updates.length === 0 && button(0).textContent.includes('K'), 'Escape preserves the old shortcut');

        await start();
        event('keydown', 'v', 'KeyV'); event('keyup', 'v', 'KeyV');
        await settle();
        check(document.querySelector('[role="alert"]').textContent.includes('not supported'), 'Plain keys show validation feedback');
        event('keydown', 'Control', 'ControlLeft', { ctrlKey: true });
        event('keydown', 'F12', 'F12', { ctrlKey: true }); event('keyup', 'F12', 'F12', { ctrlKey: true });
        check(updates.length === 0, 'Reserved keys are not saved');
        event('keydown', 'F10', 'F10', { ctrlKey: true });
        event('keyup', 'F10', 'F10', { ctrlKey: true }); event('keyup', 'Control', 'ControlLeft');
        await wait(() => updates[0] === 'Ctrl+F10' && calls.at(-1) === false);
        updates.length = 0;

        await start(); document.getElementById('outside').focus();
        await wait(() => calls.at(-1) === false);
        check(updates.length === 0, 'Moving focus cancels without saving');
        await start();
        check(!event('keydown', 'Tab', 'Tab').defaultPrevented, 'Tab can leave the recorder');
        await wait(() => calls.at(-1) === false);
        await start(); window.dispatchEvent(new Event('blur'));
        await wait(() => calls.at(-1) === false);
        await start(); window.disable(true);
        await wait(() => button(0).disabled && calls.at(-1) === false);
        window.disable(false); await wait(() => !button(0).disabled);

        await start();
        event('keydown', 'F10', 'F10', { ctrlKey: true });
        event('keyup', 'F10', 'F10');
        await wait(() => calls.at(-1) === false);
        check(updates.length === 0, 'Recording the current shortcut does not save again');
        await start(); window.changeValue('Alt+7');
        await wait(() => button(0).textContent.includes('7') && calls.at(-1) === false);

        delayStart = true;
        button(0).focus(); button(0).click();
        await wait(() => releaseStart);
        event('keydown', 'v', 'KeyV', { ctrlKey: true }); event('keyup', 'v', 'KeyV');
        check(updates.length === 0, 'Keys before native acknowledgement cannot save');
        button(1).focus(); button(1).click();
        delayStart = false; releaseStart();
        await wait(() => button(1).textContent.includes('Press a shortcut'));
        check(calls.slice(-3).join(',') === 'true,false,true', 'Old recorder restores before the next recorder starts');
        window.dispatchEvent(new Event('blur')); await wait(() => calls.at(-1) === false);

        failStart = true;
        button(0).focus(); button(0).click();
        await wait(() => document.querySelector('[role="alert"]')?.textContent.includes('Could not start') && calls.at(-1) === false);
        check(button(0).getAttribute('aria-pressed') === 'false', 'Native failure exits recording and restores shortcuts');
        failStart = false;
        await start(); root.unmount(); await wait(() => calls.at(-1) === false);
        const saved = updates.length;
        window.dispatchEvent(new KeyboardEvent('keydown', { key: 'v', code: 'KeyV', ctrlKey: true }));
        check(updates.length === saved && errors.length === 0, 'Unmount removes listeners without browser errors');
        document.getElementById('result').textContent = 'PASS';
      } catch (error) { document.getElementById('result').textContent = 'FAIL: ' + error.message; }
    ` },
    plugins: [{ name: "native-contract", setup(build) {
      build.onResolve({ filter: /^@tauri-apps\/api\/core$/ }, args => ({ path: args.path, namespace: "mock-native" }));
      build.onLoad({ filter: /.*/, namespace: "mock-native" }, () => ({ contents:
        "export const isTauri = () => true; export const invoke = (command, args) => window.nativeInvoke(command, args);" }));
    } }],
    bundle: true, write: false, outfile: "shortcut-test.js", format: "esm", platform: "browser", jsx: "automatic",
  });
  const directory = mkdtempSync(join(testDirectory, ".shortcut-browser-"));
  try {
    const html = join(directory, "test.html");
    const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
    const stylesheet = bundle.outputFiles.find(file => file.path.endsWith(".css")).text;
    writeFileSync(html, '<meta charset="utf-8"><style>' + stylesheet + '</style><div id="root"></div><pre id="result">PENDING</pre>'
      + '<script type="module">' + javascript.replace(/<\/script/gi, "<\\/script") + '</script>');
    const result = spawnSync(browser, [
      "--headless=new", "--dump-dom", "--virtual-time-budget=10000", "--disable-gpu", "--no-first-run",
      "--window-size=480,760", "--force-device-scale-factor=1", "--disable-background-networking", "--disable-extensions",
      "--user-data-dir=" + join(directory, "profile"), pathToFileURL(html).href,
    ], { encoding: "utf8", timeout: 30000, maxBuffer: 8 * 1024 * 1024 });
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    const report = result.stdout.match(/<pre id="result">([^<]*)<\/pre>/)?.[1];
    assert.equal(report, "PASS", report || result.stderr);
  } finally { rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }); }
});
