import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const testDirectory = dirname(fileURLToPath(import.meta.url));
const browser = [process.env.PROGRAMFILES, process.env["PROGRAMFILES(X86)"], process.env.LOCALAPPDATA]
  .filter(Boolean).flatMap(root => [
    join(root, "Google/Chrome/Application/chrome.exe"),
    join(root, "Microsoft/Edge/Application/msedge.exe"),
  ]).find(existsSync);

for (const width of [1200, 861, 860, 681, 680, 380]) {
  test(`audio device dropdowns preserve settings and fit a ${width}px viewport`, {
    timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
  }, async () => {
    const bundle = await build({
      stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
        import React, { useState } from 'react';
        import { createRoot } from 'react-dom/client';
        import { flushSync } from 'react-dom';
        import i18next from 'i18next';
        import { I18nextProvider } from 'react-i18next';
        import en from './src/i18n/locales/en-US.json';
        import zh from './src/i18n/locales/zh-CN.json';
        import { AudioSettingsSection } from './src/settings/sections/AudioSettingsSection';
        import { AudioStep } from './src/onboarding/steps/AudioStep';
        import './src/styles.css';
        window.requestAnimationFrame = callback => window.setTimeout(() => callback(performance.now()), 16);
        window.cancelAnimationFrame = id => window.clearTimeout(id);
        const i18n = i18next.createInstance();
        await i18n.init({ lng: 'en-US', resources: { 'en-US': en, 'zh-CN': zh }, interpolation: { escapeValue: false } });
        document.documentElement.lang = 'en-US';
        const outputDevices = [
          { id: 0, name: 'USB Headphones ' + 'Long device name '.repeat(12), is_default: true, is_loopback: true, sample_rate: 48000, channels: 2 },
          { id: 1, name: 'Display audio', is_default: false, is_loopback: true, sample_rate: 44100, channels: 2 },
        ];
        const microphoneDevices = [
          { id: 2, name: 'USB microphone', is_default: true, is_loopback: false, sample_rate: 48000, channels: 1 },
          { id: 3, name: 'Headset microphone', is_default: false, is_loopback: false, sample_rate: 44100, channels: 1 },
        ];
        const initial = { audio: {
          output: { mode: 'system', device_id: null, trigger_threshold_dbfs: -38,
            player_audio: { pagefile_drive: 'D:', opus_library_path: '', max_players: 3 } },
          microphone: { mode: 'default', device_id: null, trigger_threshold_dbfs: -42 },
        }, untouched: 'keep' };
        const updates = [];
        const reviewed = [];
        const noop = async () => {};
        let draft;
        let releaseMicrophoneTest;
        const startMicrophoneTest = () => new Promise(resolve => { releaseMicrophoneTest = resolve; });
        function Harness({ onboarding = false, busy = false, ready = true, empty = false, settings = initial }) {
          const [current, setDraft] = useState(settings);
          draft = current;
          const applySettings = update => {
            const next = update(current);
            updates.push(next);
            setDraft(next);
          };
          const devices = { outputDevices: empty ? [] : outputDevices, microphoneDevices: empty ? [] : microphoneDevices,
            devicesReady: ready, deviceErrors: [] };
          return <I18nextProvider i18n={i18n}>{onboarding
            ? <AudioStep settings={current} {...devices} hasAudioSource={true} audioBusy={busy}
                onRefreshDevices={noop} onUpdateSettings={applySettings} onSetMicrophoneReviewed={value => reviewed.push(value)} />
            : <AudioSettingsSection draft={current} {...devices} devices={[...devices.outputDevices, ...devices.microphoneDevices]}
                microphoneRunning={false} transcriptionRunning={false} microphoneTestRunning={false} saveState="saved"
                onRefresh={noop} applySettings={applySettings} onStartMicrophoneTest={startMicrophoneTest} onStopMicrophoneTest={noop} />
          }</I18nextProvider>;
        }
        const root = createRoot(document.getElementById('root'));
        const check = (condition, message) => { if (!condition) throw new Error(message); };
        const settle = () => new Promise(resolve => setTimeout(resolve, 20));
        const wait = async predicate => {
          for (let attempt = 0; attempt < 150; attempt++) { if (predicate()) return; await settle(); }
          throw new Error('Timed out: ' + predicate.toString());
        };
        const click = element => flushSync(() => element.click());
        const groups = () => [...document.querySelectorAll('.device-group')];
        const trigger = index => groups()[index].querySelector('[aria-haspopup="listbox"]');
        const menu = () => document.querySelector('[role="listbox"][aria-hidden="false"]');
        const options = () => [...menu().querySelectorAll('[role="option"]')];
        const open = async index => {
          trigger(index).scrollIntoView({ block: 'center' });
          click(trigger(index));
          await wait(menu);
        };
        const select = async (index, label) => {
          await open(index);
          const option = options().find(element => element.querySelector('.dropdown-option-copy > span')?.textContent === label);
          check(option, 'The requested audio source is available: ' + label);
          click(option);
          await wait(() => !menu());
        };
        const key = (element, value) => flushSync(() => element.dispatchEvent(new KeyboardEvent('keydown', { key: value, bubbles: true, cancelable: true })));
        const checkEdges = (first, second, message) => {
          check(Math.abs(first.left - second.left) < 1 && Math.abs(first.right - second.right) < 1,
            message + ': [' + first.left + ', ' + first.right + '] vs [' + second.left + ', ' + second.right + ']');
        };
        const checkLayout = () => {
          check(document.documentElement.scrollWidth <= innerWidth, 'The audio settings fit the viewport');
          let firstSelector;
          for (const group of groups()) {
            check(!group.querySelector('input[type="radio"]'), 'Audio devices use a dropdown instead of radio rows');
            const bounds = group.querySelector('[aria-haspopup="listbox"]').getBoundingClientRect();
            check(bounds.width > 0 && bounds.left >= 0 && bounds.right <= innerWidth, 'Each device selector fits the viewport');
            if (firstSelector) checkEdges(bounds, firstSelector, 'Both audio selectors share the same content column');
            else firstSelector = bounds;
            const title = group.querySelector('.device-group-title').getBoundingClientRect();
            if (innerWidth > 860) {
              check(bounds.left >= title.right, 'The device selector sits to the right of its title');
              check(Math.abs((bounds.top + bounds.bottom - title.top - title.bottom) / 2) < 1, 'The title and selector are vertically aligned');
            } else {
              check(bounds.top >= title.bottom, 'The narrow layout stacks the selector below its title');
            }
            for (const details of group.querySelectorAll('.microphone-level-setting, .microphone-calibration-card, .player-audio-settings, .device-empty')) {
              const detailBounds = details.getBoundingClientRect();
              check(detailBounds.top >= bounds.bottom, 'Level controls stay below the title and selector');
              checkEdges(bounds, detailBounds, 'Device details align with the selector');
            }
          }
        };
        const checkMenuLayout = () => {
          const bounds = menu().getBoundingClientRect();
          check(bounds.left >= 0 && bounds.right <= innerWidth && bounds.top >= 0 && bounds.bottom <= innerHeight, 'The menu fits the viewport');
          for (const option of options()) {
            const optionBounds = option.getBoundingClientRect();
            check(optionBounds.left >= bounds.left && optionBounds.right <= bounds.right, 'Long device names do not push menu options outside the menu');
            const selectedMark = option.querySelector('svg');
            if (selectedMark) check(selectedMark.getBoundingClientRect().right <= bounds.right, 'The selected device checkmark remains visible');
          }
        };
        const checkPlayerFields = () => {
          const card = document.querySelector('.player-audio-settings');
          const fields = ['drive', 'opus', 'limit', 'visibleLimit'].map(key => {
            const control = [...card.querySelectorAll('input[aria-label], button[aria-label]')]
              .find(element => element.getAttribute('aria-label') === i18n.t('settings.audio.players.' + key));
            return control.closest('.field').getBoundingClientRect();
          });
          if (innerWidth > 860) {
            for (const [left, right] of [[fields[0], fields[1]], [fields[2], fields[3]]]) {
              check(Math.abs(left.top - right.top) < 1 && right.left >= left.right, 'Player settings pair the drive with Opus and both player counts on aligned rows');
            }
            check(fields[2].top >= fields[0].bottom && fields[3].top >= fields[1].bottom, 'Both player counts sit below the storage fields');
            checkEdges(fields[0], fields[2], 'The drive and processing count share the left column');
            checkEdges(fields[1], fields[3], 'Opus and the visible count share the right column');
          } else {
            for (let index = 1; index < fields.length; index++) {
              check(fields[index].top >= fields[index - 1].bottom, 'Narrow player settings stack in reading order without overlap');
              checkEdges(fields[0], fields[index], 'Narrow player fields share a full-width column');
            }
          }
        };
        let stage = '';
        try {
          stage = 'settings defaults';
          flushSync(() => root.render(<Harness />));
          await settle();
          check(groups().length === 2 && groups().every(group => group.querySelector('[aria-haspopup="listbox"]')), 'Both audio sources provide a dropdown');
          checkLayout();
          check(trigger(0).textContent === i18n.t('settings.audio.systemOutput'), 'The default output is shown');
          check(trigger(1).textContent === i18n.t('settings.audio.defaultMicrophone'), 'The default microphone is shown');
          check(updates.length === 0, 'Rendering does not save settings');
          await open(0);
          await new Promise(resolve => setTimeout(resolve, 250));
          checkMenuLayout();
          check(options().length === 6, 'All four output modes and both hardware devices remain available');
          check(options().find(option => option.textContent.includes(outputDevices[0].name))?.textContent.includes('48,000 Hz'), 'Device format details remain available');
          click(options().find(option => option.textContent.includes(outputDevices[0].name)));
          await wait(() => !menu());
          check(draft.audio.output.mode === 'system' && draft.audio.output.device_id === 0, 'Device ID zero selects the hardware output');
          check(trigger(0).textContent === outputDevices[0].name, 'The saved device is shown');
          checkLayout();
          const previousUpdates = updates.length;
          await select(0, outputDevices[0].name);
          check(updates.length === previousUpdates, 'Selecting the current device does not resave settings');
          await select(0, 'VRChat');
          check(draft.audio.output.mode === 'vrchat' && draft.audio.output.device_id === null, 'VRChat clears the hardware device');
          await select(0, i18n.t('settings.audio.players.title'));
          check(draft.audio.output.mode === 'vrchat_players' && document.querySelector('.player-audio-settings'), 'Player audio keeps its settings panel');
          checkLayout();
          checkPlayerFields();
          await select(0, i18n.t('settings.audio.disableOtherVoices'));
          check(draft.audio.output.mode === 'disabled' && !document.querySelector('.player-audio-settings'), 'Disabling output hides player audio settings');
          await select(0, i18n.t('settings.audio.systemOutput'));
          check(draft.audio.output.mode === 'system' && draft.audio.output.device_id === null, 'System default clears the hardware device');

          stage = 'microphone selection and keyboard';
          await select(1, microphoneDevices[0].name);
          check(draft.audio.microphone.mode === 'device' && draft.audio.microphone.device_id === 2, 'Microphone selection saves its numeric ID');
          await select(1, i18n.t('settings.audio.disableMicrophone'));
          check(draft.audio.microphone.mode === 'disabled' && draft.audio.microphone.device_id === null, 'Disabling the microphone clears its device');
          await select(1, i18n.t('settings.audio.defaultMicrophone'));
          check(draft.audio.microphone.mode === 'default' && draft.audio.microphone.device_id === null, 'Default microphone selection is preserved');
          trigger(1).focus();
          key(trigger(1), 'ArrowDown');
          await wait(() => menu()?.contains(document.activeElement));
          key(document.activeElement, 'End');
          check(document.activeElement.textContent.includes('Headset microphone'), 'Keyboard navigation reaches the last microphone');
          click(document.activeElement);
          await wait(() => document.activeElement === trigger(1));
          check(draft.audio.microphone.device_id === 3, 'The keyboard-focused microphone can be selected');
          key(trigger(1), 'ArrowDown');
          await wait(menu);
          key(trigger(1), 'Escape');
          await wait(() => !menu());
          click(document.querySelector('.microphone-test-button'));
          check(trigger(1).disabled, 'A pending microphone operation disables device selection');
          releaseMicrophoneTest();
          await wait(() => !trigger(1).disabled);
          check(updates.every(next => next.untouched === 'keep' && next.audio.output.trigger_threshold_dbfs === -38
            && next.audio.microphone.trigger_threshold_dbfs === -42 && next.audio.output.player_audio.max_players === 3), 'Source changes preserve thresholds and other settings');

          stage = 'missing devices and scan state';
          const missing = { ...initial, audio: { ...initial.audio, output: { ...initial.audio.output, device_id: 99 } } };
          const savedCount = updates.length;
          flushSync(() => root.render(<Harness key="missing" settings={missing} empty={true} ready={false} />));
          checkLayout();
          check(trigger(0).textContent === i18n.t('settings.audio.scanning'), 'A missing selection shows scan progress rather than a false default');
          check(groups()[0].textContent.includes(i18n.t('settings.audio.scanning')), 'Scan progress is visible');
          flushSync(() => root.render(<Harness key="missing" settings={missing} empty={true} />));
          checkLayout();
          check(trigger(0).textContent === i18n.t('common.unavailable'), 'A missing device is not presented as the system default');
          check(groups()[0].textContent.includes(i18n.t('settings.audio.noDevices')), 'An empty scan has an explicit result');
          check(updates.length === savedCount && draft.audio.output.device_id === 99, 'Scanning does not overwrite the saved source');
          await select(0, i18n.t('settings.audio.systemOutput'));
          check(draft.audio.output.device_id === null, 'Default sources remain selectable without hardware results');

          stage = 'shared onboarding selector';
          flushSync(() => root.render(<Harness key="onboarding" onboarding={true} busy={true} />));
          check(trigger(0).disabled && trigger(1).disabled, 'Onboarding busy state disables both selectors');
          flushSync(() => root.render(<Harness key="onboarding" onboarding={true} />));
          await select(0, outputDevices[0].name);
          await select(1, i18n.t('settings.audio.disableMicrophone'));
          check(reviewed.at(-1) === true, 'Disabled microphones remain reviewed in onboarding');
          await select(1, microphoneDevices[0].name);
          check(reviewed.at(-1) === false, 'Changing the onboarding microphone resets its review');
          checkLayout();

          stage = 'open menu layout';
          flushSync(() => root.render(<Harness key="preview" />));
          await open(0);
          await new Promise(resolve => setTimeout(resolve, 250));
          checkMenuLayout();
          checkLayout();
          const closingMenu = menu();
          key(trigger(0), 'Escape');
          // Headless virtual time does not advance the Web Animations timeline.
          closingMenu.getAnimations().forEach(animation => animation.finish());
          await wait(() => !document.querySelector('[role="listbox"]'));
          trigger(0).blur();

          stage = 'localized settings layout';
          await i18n.changeLanguage('zh-CN');
          document.documentElement.lang = 'zh-CN';
          flushSync(() => root.render(<Harness key="player-preview" settings={{ ...initial, audio: {
            ...initial.audio, output: { ...initial.audio.output, mode: 'vrchat_players' },
          } }} />));
          await new Promise(resolve => setTimeout(resolve, 250));
          checkLayout();
          checkPlayerFields();
          document.getElementById('geometry').textContent = JSON.stringify(groups().map(group => {
            const edges = element => {
              const bounds = element.getBoundingClientRect();
              return { left: bounds.left, right: bounds.right };
            };
            return { selector: edges(group.querySelector('[aria-haspopup="listbox"]')),
              level: edges(group.querySelector('.microphone-level-setting')) };
          }));
          document.getElementById('result').textContent = 'PASS';
        } catch (error) { document.getElementById('result').textContent = 'FAIL: ' + stage + ': ' + error.message; }
        parent.postMessage({ result: document.getElementById('result').textContent, geometry: document.getElementById('geometry').textContent }, '*');
      ` },
      plugins: [{ name: "locale-fixture", setup(build) {
        build.onLoad({ filter: /[\\/]i18n[\\/]catalog\.ts$/ }, () => ({
          contents: 'import en from "./src/i18n/locales/en-US.json"; export const localeCatalog = [en]; export const supportedUiLocales = ["en-US"]; export const localeResources = { "en-US": { translation: en.translation } };',
          resolveDir: join(testDirectory, ".."), loader: "ts",
        }));
      } }],
      define: { "import.meta.env": "{}" },
      bundle: true, write: false, outfile: "audio-device-test.js", format: "esm", platform: "browser", jsx: "automatic",
    });
    const directory = mkdtempSync(join(testDirectory, ".audio-device-browser-"));
    try {
      const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
      const stylesheet = bundle.outputFiles.find(file => file.path.endsWith(".css")).text;
      writeFileSync(join(directory, "content.html"), '<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><style>' + stylesheet
        + '</style><div id="root" class="settings-surface"></div><pre id="result" hidden>PENDING</pre><pre id="geometry" hidden></pre>'
        + '<script>const fail = message => { document.getElementById("result").textContent = "FAIL: " + message; parent.postMessage("FAIL: " + message, "*"); }; window.addEventListener("error", event => fail(event.message)); window.addEventListener("unhandledrejection", event => fail(String(event.reason)));</script>'
        + '<script type="module">' + javascript.replace(/<\/script/gi, "<\\/script") + '</script>');
      const html = join(directory, "test.html");
      writeFileSync(html, '<body style="margin:0"><iframe src="content.html" style="border:0;width:' + width
        + 'px;height:1000px"></iframe><pre id="result" hidden>PENDING</pre><pre id="geometry" hidden></pre><script>window.addEventListener("message", event => { document.getElementById("result").textContent = event.data.result ?? event.data; document.getElementById("geometry").textContent = event.data.geometry ?? ""; });</script></body>');
      const screenshot = resolve(testDirectory, '../../../core/target/audio-device-dropdown-' + width + '.png');
      mkdirSync(dirname(screenshot), { recursive: true });
      const result = spawnSync(browser, [
        "--headless=new", "--dump-dom", "--virtual-time-budget=15000", "--disable-gpu", "--no-first-run",
        "--disable-background-networking", "--disable-extensions", "--window-size=" + Math.max(500, width) + ",1100",
        "--screenshot=" + screenshot, "--user-data-dir=" + join(directory, "profile"), pathToFileURL(html).href,
      ], { encoding: "utf8", windowsHide: true, timeout: 30000, maxBuffer: 8 * 1024 * 1024 });
      assert.ifError(result.error);
      assert.equal(result.status, 0, result.stderr);
      const report = result.stdout.match(/<pre id="result"[^>]*>([^<]*)<\/pre>/)?.[1];
      assert.equal(report, "PASS", report || result.stderr);
      console.log(width + 'px alignment:', result.stdout.match(/<pre id="geometry"[^>]*>([^<]*)<\/pre>/)?.[1]);
    } finally {
      assert.equal(dirname(resolve(directory)), testDirectory);
      assert(basename(directory).startsWith(".audio-device-browser-"));
      rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
    }
  });
}
