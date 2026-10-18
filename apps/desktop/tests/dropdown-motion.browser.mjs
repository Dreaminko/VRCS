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

for (const [width, reduced] of [[1200, false], [380, false], [1200, true]]) {
  test(`dropdown motion preserves dismissal, focus and placement at ${width}px (reduced: ${reduced})`, {
    timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
  }, async () => {
    const bundle = await build({
      stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
        import React, { useState } from 'react';
        import { createRoot } from 'react-dom/client';
        import { flushSync } from 'react-dom';
        import i18next from 'i18next';
        import { initReactI18next } from 'react-i18next';
        import { DropdownField, EditableDropdownField } from './src/shared/ui/DropdownField';
        import { DeckTreeSelect } from './src/settings/SettingsControls';
        import en from './src/i18n/locales/en-US.json';
        import './src/styles.css';
        await i18next.use(initReactI18next).init({ lng: 'en-US', resources: { 'en-US': { translation: en.translation } } });
        window.requestAnimationFrame = callback => window.setTimeout(() => callback(performance.now()), 16);
        window.cancelAnimationFrame = id => window.clearTimeout(id);
        const options = [{ value: 'first', label: 'First option' }, { value: 'second', label: 'Second option' }];
        function Harness() {
          const [value, setValue] = useState('first');
          const [disabled, setDisabled] = useState(false);
          window.disableFields = setDisabled;
          return <div style={{ padding: 24 }}>
            <button id="outside">Outside</button>
            {['plain', 'search', 'floating', 'above'].map(id => <section key={id} id={id}
              style={id === 'above' ? { position: 'fixed', bottom: 24, left: 24, width: 260 } : { width: 260, marginTop: 24 }}>
              <DropdownField label={id} value={value} options={options} disabled={disabled}
                searchable={id === 'search'} floating={id === 'floating' || id === 'above'} onChange={setValue} />
            </section>)}
            <section id="editable" style={{ width: 260, marginTop: 24 }}><EditableDropdownField
              label="Editable" value={value} options={options} disabled={disabled} onChange={setValue} /></section>
            <section id="tree" style={{ width: 260, marginTop: 24 }}><DeckTreeSelect
              label="Deck" value="First" decks={['First', 'Second']} disabled={disabled} onChange={() => {}} /></section>
          </div>;
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
        const key = (element, value) => flushSync(() => element.dispatchEvent(new KeyboardEvent('keydown', { key: value, bubbles: true })));
        const getMenu = () => document.querySelector('.dropdown-menu, .deck-tree-menu');
        let stage = '';
        const animations = menu => menu.getAnimations().map(animation => ({
          type: animation.constructor.name, state: animation.playState, time: animation.currentTime,
          property: animation.transitionProperty, name: animation.animationName, timing: animation.effect.getTiming(),
        }));
        const finish = async menu => {
          for (const animation of menu.getAnimations()) animation.finish();
          try { await wait(() => !menu.isConnected || !menu.getAnimations().length); }
          catch (error) { throw new Error(stage + ': ' + JSON.stringify(animations(menu))); }
        };
        try {
          check(matchMedia('(prefers-reduced-motion: reduce)').matches === ${reduced}, 'Expected motion preference');
          for (const id of ['plain', 'search', 'floating', 'above', 'editable', 'tree']) {
            stage = id + ' entry';
            const trigger = document.querySelector('#' + id + ' .dropdown-trigger, #' + id + ' .editable-dropdown-toggle');
            click(trigger);
            await wait(() => getMenu());
            const menu = getMenu();
            check(trigger.getAttribute('aria-expanded') === 'true' && !menu.inert, id + ': opening is interactive immediately');
            if (!${reduced}) {
              const entry = menu.getAnimations()[0];
              check(entry, id + ': entry animation exists');
              entry.pause();
              entry.currentTime = entry.effect.getTiming().duration / 2;
              const opacity = Number(getComputedStyle(menu).opacity);
              check(opacity > 0.7 && opacity < 1, id + ': entry eases to rest');
            } else check(!menu.getAnimations().length, id + ': reduced motion skips animation: ' + JSON.stringify(animations(menu)));
            await finish(menu);
            const before = menu.getBoundingClientRect();
            const anchor = trigger.getBoundingClientRect();
            if (id === 'floating') check(before.top >= anchor.bottom, 'Floating menu opens below its trigger');
            if (id === 'above') {
              check(before.bottom <= anchor.top, 'Menu near the bottom opens above');
              check(getComputedStyle(menu).transformOrigin.endsWith(before.height + 'px'), 'Above menu scales from its bottom edge');
            }
            click(trigger);
            stage = id + ' exit';
            check(trigger.getAttribute('aria-expanded') === 'false', id + ': dismissal updates immediately');
            if (!${reduced}) {
              check(menu.isConnected && menu.inert && menu.getAttribute('aria-hidden') === 'true', id + ': exiting menu stays visible but cannot receive input');
              const exit = menu.getAnimations()[0];
              exit.pause();
              exit.currentTime = 0;
              const after = menu.getBoundingClientRect();
              check(Math.abs(before.left - after.left) <= 1 && Math.abs(before.top - after.top) <= 1, id + ': exit keeps its position');
              exit.currentTime = exit.effect.getTiming().duration / 2;
              check(Number(getComputedStyle(menu).opacity) > 0.5, id + ': exit accelerates');
              const opacity = getComputedStyle(menu).opacity;
              click(trigger);
              stage = id + ' reopen';
              check(getMenu() === menu && !menu.inert, id + ': reopening reuses the live surface');
              const reopen = menu.getAnimations()[0];
              reopen.pause();
              reopen.currentTime = 0;
              check(Math.abs(Number(getComputedStyle(menu).opacity) - Number(opacity)) < 0.02, id + ': interrupted motion does not snap');
              await finish(menu);
              click(trigger);
              stage = id + ' final exit';
              await finish(menu);
            }
            await wait(() => !getMenu());
          }
          const trigger = document.querySelector('#search .dropdown-trigger');
          key(trigger, 'ArrowDown');
          await wait(() => document.activeElement?.classList.contains('dropdown-search'));
          let menu = getMenu();
          await finish(menu);
          const input = document.activeElement;
          flushSync(() => {
            Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, 'Second');
            input.dispatchEvent(new Event('input', { bubbles: true }));
          });
          key(input, 'ArrowDown');
          check(document.activeElement.textContent.includes('Second option'), 'Keyboard navigation follows filtered options');
          key(document.activeElement, 'Escape');
          await finish(menu);
          await wait(() => document.activeElement === trigger);
          click(trigger);
          await wait(() => getMenu());
          menu = getMenu();
          await finish(menu);
          check(menu.querySelector('input').value === '', 'Search resets after dismissal');
          click(menu.querySelectorAll('[role=option]')[1]);
          await finish(menu);
          check(trigger.textContent.includes('Second option'), 'Selection applies before focus returns');
          await wait(() => document.activeElement === trigger);
          click(trigger);
          await wait(() => getMenu());
          menu = getMenu();
          await finish(menu);
          flushSync(() => document.getElementById('outside').dispatchEvent(new PointerEvent('pointerdown', { bubbles: true })));
          await finish(menu);
          check(!getMenu(), 'Outside click dismisses the menu');
          click(trigger);
          await wait(() => getMenu());
          menu = getMenu();
          await finish(menu);
          flushSync(() => window.disableFields(true));
          await wait(() => trigger.getAttribute('aria-expanded') === 'false');
          await finish(menu);
          check(!getMenu() && trigger.disabled, 'Disabling a field completes dismissal');
          root.unmount();
          document.getElementById('result').textContent = 'PASS';
        } catch (error) { document.getElementById('result').textContent = 'FAIL: ' + error.message; }
        parent.postMessage(document.getElementById('result').textContent, '*');
      ` },
      bundle: true, write: false, outfile: "dropdown-motion-test.js", format: "esm", platform: "browser", jsx: "automatic",
      define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
    });
    const directory = mkdtempSync(join(testDirectory, ".dropdown-motion-browser-"));
    try {
      const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
      const stylesheet = bundle.outputFiles.find(file => file.path.endsWith(".css")).text;
      writeFileSync(join(directory, "content.html"), '<style>' + stylesheet
        + '</style><div id="root" class="settings-surface"></div><pre id="result">PENDING</pre><script type="module">'
        + javascript.replace(/<\/script/gi, "<\\/script") + '</script>');
      const html = join(directory, "test.html");
      writeFileSync(html, '<body style="margin:0"><iframe src="content.html" style="border:0;width:' + width
        + 'px;height:900px"></iframe><pre id="result">PENDING</pre><script>window.addEventListener("message", event => document.getElementById("result").textContent = event.data);</script></body>');
      const result = spawnSync(browser, [
        "--headless=new", "--dump-dom", "--virtual-time-budget=15000", "--disable-gpu", "--no-first-run",
        "--disable-background-networking", "--disable-extensions", "--window-size=" + Math.max(500, width) + ",900",
        ...(reduced ? ["--force-prefers-reduced-motion=reduce"] : []),
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
