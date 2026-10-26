import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
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
  test(width === 1200 ? "model autocomplete preserves typing, selection and custom names"
    : "model autocomplete menus stay within the narrow viewport", {
    timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
  }, async () => {
    const bundle = await build({
      stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
        import React, { useState } from 'react';
        import { createRoot } from 'react-dom/client';
        import { flushSync } from 'react-dom';
        import { EditableDropdownField } from './src/shared/ui/DropdownField';
        import './src/styles.css';
        window.requestAnimationFrame = callback => window.setTimeout(() => callback(performance.now()), 16);
        window.cancelAnimationFrame = id => window.clearTimeout(id);
        const options = [
          { value: 'gpt-5-mini', label: 'GPT mini' },
          { value: 'deepseek-v3', label: 'DeepSeek V3' },
          { value: 'provider/deepseek-r1', label: 'Reasoner' },
        ];
        let updates = [];
        let setDisabled;
        let setOptionsDisabled;
        let setOptions;
        let replaceValue;
        let escaped = 0;
        function Harness({ floating, commitOnBlur }) {
          const [value, setValue] = useState('gpt-5-mini');
          const [disabled, updateDisabled] = useState(false);
          const [optionsDisabled, updateOptionsDisabled] = useState(false);
          const [catalog, updateOptions] = useState(options);
          setDisabled = updateDisabled;
          setOptionsDisabled = updateOptionsDisabled;
          setOptions = updateOptions;
          replaceValue = setValue;
          return <div style={{ width: 'min(300px, calc(100vw - 48px))', margin: 24 }}
            onKeyDown={event => { if (event.key === 'Escape') escaped++; }}>
            <EditableDropdownField label="Model" value={value} options={catalog} disabled={disabled}
              optionsDisabled={optionsDisabled} floating={floating ? undefined : false} commitOnBlur={commitOnBlur}
              onChange={next => { updates.push(next); setValue(next); }} />
          </div>;
        }
        const root = createRoot(document.getElementById('root'));
        const check = (condition, message) => { if (!condition) throw new Error(message); };
        const settle = () => new Promise(resolve => setTimeout(resolve, 20));
        const wait = async predicate => {
          for (let i = 0; i < 150; i++) { if (predicate()) return; await settle(); }
          throw new Error('Timed out: ' + predicate.toString());
        };
        const key = (input, key, isComposing = false) => flushSync(() =>
          input.dispatchEvent(new KeyboardEvent('keydown', { key, isComposing, bubbles: true })));
        const type = (input, value) => flushSync(() => {
          input.focus();
          Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, value);
          input.dispatchEvent(new Event('input', { bubbles: true }));
        });
        const outside = () => flushSync(() => document.getElementById('outside').focus());
        let stage = '';
        try {
          check(innerWidth === ${width}, 'The test uses the expected CSS viewport');
          for (const floating of [false, true]) for (const commitOnBlur of ${width === 1200 ? "[false, true]" : "[false]"}) {
            stage = JSON.stringify({ floating, commitOnBlur });
            updates = [];
            flushSync(() => root.render(<Harness key={stage} floating={floating} commitOnBlur={commitOnBlur} />));
            const input = document.querySelector('input[role=combobox]');
            const toggle = document.querySelector('.editable-dropdown-toggle');
            const menu = () => document.getElementById(input.getAttribute('aria-controls'));
            const visible = () => [...menu().querySelectorAll('[role=option]')];
            const active = () => document.getElementById(input.getAttribute('aria-activedescendant'));
            if (${width !== 1200}) {
              flushSync(() => toggle.click());
              await wait(() => menu() && !menu().inert);
              for (const animation of document.getAnimations()) {
                if (animation.effect.getTiming().iterations !== Infinity) animation.finish();
              }
              await settle();
              for (const element of [input, toggle, menu(), ...visible()]) {
                const bounds = element.getBoundingClientRect();
                check(bounds.width > 0 && bounds.height > 0, 'Menu controls have visible bounds');
                check(bounds.left >= 0 && bounds.right <= innerWidth + 1, 'Menu controls stay within the viewport width');
                check(bounds.top >= 0 && bounds.bottom <= innerHeight + 1, 'Menu controls stay within the viewport height');
              }
              check(menu().scrollWidth <= menu().clientWidth + 1, 'Suggestions do not overflow the menu horizontally');
              continue;
            }
            type(input, '  DEEPS  ');
            check(input.getAttribute('aria-expanded') === 'true', 'Typing opens matching model suggestions');
            await wait(() => menu());
            check(visible().length === 2, 'Search matches model labels and provider-prefixed IDs without case sensitivity');
            check(document.activeElement === input, 'Filtering keeps focus in the input');
            if (commitOnBlur) check(updates.length === 0, 'Filtering does not save an unfinished model');
            key(input, 'ArrowDown');
            check(active()?.textContent.includes('DeepSeek V3'), 'Down activates the first match');
            key(input, 'ArrowDown');
            check(active()?.textContent.includes('Reasoner'), 'Down follows the filtered results');
            key(input, 'Enter', true);
            check(input.value === '  DEEPS  ', 'IME Enter does not select a suggestion');
            updates = [];
            key(input, 'Enter');
            check(input.value === 'provider/deepseek-r1', 'Enter applies the active model ID');
            check(JSON.stringify(updates) === JSON.stringify(['provider/deepseek-r1']), 'Selection saves only the chosen model');
            check(document.activeElement === input && input.getAttribute('aria-expanded') === 'false', 'Selection closes suggestions and keeps input focus');
            outside();
            check(updates.length === 1, 'Leaving after selection does not save twice');

            type(input, 'deeps');
            await wait(() => menu() && !menu().inert);
            const escapesBefore = escaped;
            key(input, 'Escape');
            check(input.value === 'deeps' && document.activeElement === input, 'Escape preserves typed text and focus');
            check(input.getAttribute('aria-expanded') === 'false' && escaped === escapesBefore, 'Escape closes suggestions without reaching the containing settings');
            type(input, 'deepseek');
            await wait(() => menu() && !menu().inert);
            updates = [];
            const option = visible()[0];
            flushSync(() => {
              const event = new MouseEvent('mousedown', { bubbles: true, cancelable: true, button: 0 });
              if (option.dispatchEvent(event)) option.focus();
            });
            flushSync(() => option.click());
            check(input.value === 'deepseek-v3', 'Mouse selection applies the model');
            check(JSON.stringify(updates) === JSON.stringify(['deepseek-v3']), 'Mouse selection does not save the search prefix first');

            type(input, 'private-custom-model');
            check(input.getAttribute('aria-expanded') === 'false', 'Unmatched custom names do not show unrelated suggestions');
            updates = [];
            key(input, 'Enter', true);
            check(document.activeElement === input && updates.length === 0, 'IME Enter does not submit custom text');
            if (commitOnBlur) {
              key(input, 'Enter');
              check(JSON.stringify(updates) === JSON.stringify(['private-custom-model']), 'Enter commits a custom model when blur commits are enabled');
            } else {
              type(input, 'another-custom-model');
              check(updates[0] === 'another-custom-model', 'Immediate fields still update during typing');
            }
            updates = [];
            flushSync(() => toggle.click());
            await wait(() => menu() && !menu().inert);
            check(visible().length === options.length, 'The toggle opens the full catalog after custom input');
            check(updates.length === 0, 'Opening the catalog does not submit custom text');
            outside();
            check(input.getAttribute('aria-expanded') === 'false', 'Leaving the field dismisses suggestions');

            type(input, 'deeps');
            await wait(() => menu() && !menu().inert);
            key(input, 'ArrowUp');
            check(active()?.textContent.includes('Reasoner'), 'Up activates the last match');
            flushSync(() => setOptions([options[1]]));
            check(!input.getAttribute('aria-activedescendant'), 'Catalog changes remove a stale active model');
            updates = [];
            key(input, 'ArrowDown');
            key(input, 'Enter');
            check(updates[0] === 'deepseek-v3', 'Keyboard selection follows the updated catalog');
            flushSync(() => replaceValue('external-model'));
            await wait(() => input.value === 'external-model');
            check(input.value === 'external-model', 'External settings updates replace the displayed model');

            type(input, 'deep');
            flushSync(() => setOptionsDisabled(true));
            await wait(() => input.getAttribute('aria-expanded') === 'false');
            check(input.getAttribute('aria-expanded') === 'false' && !input.disabled && toggle.disabled, 'Catalog loading hides suggestions but permits manual input');
            type(input, 'manual-during-loading');
            check(input.value === 'manual-during-loading', 'Catalog loading does not block custom input');
            flushSync(() => setOptionsDisabled(false));
            type(input, 'deep');
            flushSync(() => setDisabled(true));
            await wait(() => input.getAttribute('aria-expanded') === 'false');
            check(input.disabled && input.getAttribute('aria-expanded') === 'false', 'Disabling the field closes suggestions');
          }
          document.getElementById('result').textContent = 'PASS';
        } catch (error) { document.getElementById('result').textContent = 'FAIL: ' + stage + ': ' + error.message; }
        parent.postMessage(document.getElementById('result').textContent, '*');
      ` },
      bundle: true, write: false, outfile: "model-autocomplete-test.js", format: "esm", platform: "browser", jsx: "automatic",
    });
    const directory = mkdtempSync(join(testDirectory, ".model-autocomplete-browser-"));
    try {
      const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
      const stylesheet = bundle.outputFiles.find(file => file.path.endsWith(".css")).text;
      const html = join(directory, "test.html");
      writeFileSync(join(directory, "content.html"), '<style>' + stylesheet + '</style><div id="root" class="settings-surface"></div>'
        + '<button id="outside">Outside</button><pre id="result">PENDING</pre><script type="module">'
        + javascript.replace(/<\/script/gi, "<\\/script") + '</script>');
      writeFileSync(html, '<body style="margin:0"><iframe src="content.html" style="border:0;width:' + width
        + 'px;height:900px"></iframe><pre id="result">PENDING</pre><script>window.addEventListener("message", event => document.getElementById("result").textContent = event.data);</script></body>');
      const result = spawnSync(browser, [
        "--headless=new", "--dump-dom", "--virtual-time-budget=15000", "--disable-gpu", "--no-first-run",
        "--disable-background-networking", "--disable-extensions", "--window-size=" + Math.max(500, width) + ",900",
        "--user-data-dir=" + join(directory, "profile"), pathToFileURL(html).href,
      ], { encoding: "utf8", timeout: 30000, maxBuffer: 8 * 1024 * 1024, windowsHide: true });
      assert.ifError(result.error);
      assert.equal(result.status, 0, result.stderr);
      const report = result.stdout.match(/<pre id="result">([^<]*)<\/pre>/)?.[1];
      assert.equal(report, "PASS", report || result.stderr);
    } finally {
      const cleanupPath = resolve(directory);
      const cleanupRelative = relative(resolve(testDirectory), cleanupPath);
      assert.ok(cleanupRelative && cleanupRelative !== ".." && !cleanupRelative.startsWith(".." + sep)
        && !isAbsolute(cleanupRelative), "Temporary directory must resolve inside the tests directory");
      rmSync(cleanupPath, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
    }
  });
}
