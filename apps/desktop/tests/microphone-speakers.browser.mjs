import assert from "node:assert/strict";
import { existsSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const { chromium } = createRequire(import.meta.url)("playwright");
const directory = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const screenshots = resolve(directory, "../../.build/microphone-speakers");
const executablePath = [process.env.PROGRAMFILES, process.env["PROGRAMFILES(X86)"], process.env.LOCALAPPDATA]
  .filter(Boolean).flatMap(root => [join(root, "Google/Chrome/Application/chrome.exe"), join(root, "Microsoft/Edge/Application/msedge.exe")]).find(existsSync);

test("microphone controls work without player audio and wrist actions preserve the capture identity", { timeout: 45000 }, async () => {
  const bundle = await build({
    stdin: { resolveDir: directory, loader: "tsx", contents: `
      import React, { useState } from 'react';
      import { createRoot } from 'react-dom/client';
      import { flushSync } from 'react-dom';
      import i18next from 'i18next';
      import { initReactI18next } from 'react-i18next';
      import locale from './src/i18n/locales/zh-CN.json';
      import { PlayerHistoryControls } from './src/subtitles/components/PlayerHistoryControls';
      import { MicrophoneSpeakerControls } from './src/capture/MicrophoneSpeakerControls';
      import { useMicrophoneSpeakers, publishMicrophoneSpeakers } from './src/capture/useMicrophoneSpeakers';
      import { VrSurface } from './src/vr-surface/VrSurface';
      import './src/styles.css';
      await i18next.use(initReactI18next).init({ lng: 'zh-CN', resources: { 'zh-CN': { translation: locale.translation } } });
      const root = createRoot(document.getElementById('root'));
      const initial = { enabled: false, supported: true, running: true, generation: 'current-mic', revision: 4, snapshot_sequence: 8,
        speakers: [{ id: 'alice', index: 0, decision: 'allow', last_active_at: new Date().toISOString() }, { id: 'bob', index: 1, decision: 'allow', last_active_at: new Date().toISOString() }] };
      window.actions = [];
      let presetSnapshot = { ...initial, supported: false, running: false, generation: '', speakers: [] };
      window.presetSettings = { features: { microphone_speaker_filter: true }, audio: { microphone: { speaker_filter_enabled: false } } };
      publishMicrophoneSpeakers(presetSnapshot);
      window.fetch = async () => new Response(JSON.stringify(presetSnapshot), { status: 200 });
      const savePreset = async apply => {
        window.presetSettings = apply(window.presetSettings);
        presetSnapshot = { ...presetSnapshot, enabled: window.presetSettings.audio.microphone.speaker_filter_enabled,
          revision: presetSnapshot.revision + 1, snapshot_sequence: presetSnapshot.snapshot_sequence + 1 };
        publishMicrophoneSpeakers(presetSnapshot);
        return window.presetSettings;
      };
      function Preset() {
        const control = useMicrophoneSpeakers(true, savePreset);
        return <MicrophoneSpeakerControls control={control} />;
      }
      window.renderPreset = () => flushSync(() => root.render(<Preset />));
      function Desktop() {
        const [snapshot, setSnapshot] = useState(initial);
        const [preferences, setPreferences] = useState({ hiddenTracks: ['old-player'], hiddenSourceLanguages: null });
        const control = { snapshot, connected: true, busy: false, error: null, getCurrent: () => snapshot,
          execute: async action => {
            window.actions.push(action);
            setSnapshot(value => ({ ...value, revision: value.revision + 1,
              ...('set_microphone_filter' in action ? { enabled: action.set_microphone_filter } : { speakers: value.speakers.map(s => s.id === action.set_microphone_speaker.id ? { ...s, decision: action.set_microphone_speaker.decision } : s) }) }));
          } };
        return <div style={{ position: 'relative', width: 480, height: '100vh', padding: 12 }}>
          <PlayerHistoryControls subtitles={[]} enabled={false} selectedUserIds={null} defaultLanguages={[]} preferences={preferences}
            onPreferences={setPreferences} onSelection={async () => { throw Error('Unexpected player selection'); }} microphone={control} />
        </div>;
      }
      window.renderDesktop = () => flushSync(() => root.render(<Desktop />));
      const micLabels = Object.fromEntries(Object.entries(locale.translation.live.microphoneSpeakers).map(([key, value]) => ['mic_' + key, value]));
      window.renderWrist = (supported = true, running = true) => {
        const microphone = { ...initial, supported, running, generation: running ? initial.generation : '', enabled: running,
          speakers: running ? Array.from({ length: 7 }, (_, index) => ({
          id: 'mic-' + index, index, decision: index ? 'block' : 'allow', name: '讲话人 ' + (index + 1), last_active: '最近活动 12:00:00' })) : [] };
        const controls = { owner_epoch: 1, revision: 1, dark: false, labels: {}, items: Array.from({ length: 4 }, (_, index) => ({ label: String(index), requested: true, effective: true, available: true, busy: false, reason: null })) };
        flushSync(() => root.render(<VrSurface state={{ key: { generation: 1, revision: supported ? 1 : 2 }, hovered: null, pressed: null, pointer: null,
          view: { kind: 'menu', controls, ui: { page: 'microphone', group: 'current', player_page: 0, language_page: 0, microphone_page: 0 }, players: {
            available: true, processing_available: false, player_mode: false, selection_mode: 'all', selected_count: 0,
            players: [], languages: [], follow_default: true, default_languages: [], microphone,
            labels: { ...locale.translation.live.players, ...micLabels }, busy: false, error: null,
          } } }} onLayout={layout => { window.layout = layout; }} />));
      };
      window.renderPreset();
    ` },
    bundle: true, write: false, outfile: "microphone.js", format: "esm", platform: "browser", jsx: "automatic", minify: true,
    define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
    loader: { ".png": "dataurl" },
  });
  const browser = await chromium.launch({ headless: true, ...(executablePath ? { executablePath } : {}) });
  try {
    const page = await browser.newPage({ viewport: { width: 520, height: 800 } });
    await page.setContent('<!doctype html><html><body><div id="root"></div></body></html>');
    await page.addStyleTag({ content: bundle.outputFiles.find(file => file.path.endsWith(".css")).text });
    await page.addScriptTag({ content: bundle.outputFiles.find(file => file.path.endsWith(".js")).text, type: "module" });
    const preset = page.getByRole("checkbox", { name: "按说话人过滤麦克风输出", exact: true });
    await preset.waitFor();
    assert.equal(await preset.isDisabled(), false, "Filtering can be enabled before capture starts");
    await preset.check();
    await page.waitForFunction(() => window.presetSettings.audio.microphone.speaker_filter_enabled);
    await page.evaluate(() => window.renderDesktop());
    await page.getByRole("button", { name: "人员与译文显示", exact: true }).click();
    const mic = page.locator(".microphone-speaker-controls");
    await mic.getByRole("checkbox").check();
    assert.equal(await mic.locator(".player-history-row").first().getByRole("button", { name: "允许", exact: true }).getAttribute("aria-pressed"), "true");
    await mic.locator(".player-history-row").nth(1).getByRole("button", { name: "屏蔽", exact: true }).click();
    assert.equal(await mic.locator(".player-history-row").nth(1).getByRole("button", { name: "屏蔽", exact: true }).getAttribute("aria-pressed"), "true");
    await page.getByRole("button", { name: "显示所有记录", exact: true }).click();
    assert.equal(await mic.getByRole("checkbox").isChecked(), true);
    assert.deepEqual(await page.evaluate(() => window.actions), [
      { set_microphone_filter: true },
      { set_microphone_speaker: { generation: "current-mic", revision: 5, id: "bob", decision: "block" } },
    ]);
    mkdirSync(screenshots, { recursive: true });
    await page.screenshot({ path: join(screenshots, "desktop.png") });
    await page.setViewportSize({ width: 1280, height: 1280 });
    await page.evaluate(() => window.renderWrist());
    await page.waitForFunction(() => window.layout);
    assert.equal(await page.locator(".vr-menu-player-row").count(), 5);
    assert.equal(await page.locator('[data-vr-menu=\'{"microphone_page":1}\']').isDisabled(), false);
    const commands = await page.locator(".vr-menu-player-row [data-vr-menu]").evaluateAll(nodes => nodes.map(node => JSON.parse(node.dataset.vrMenu)));
    assert.deepEqual(commands[1], { display: { set_microphone_speaker: { generation: "current-mic", revision: 4, id: "mic-0", decision: "block" } } });
    const regions = await page.evaluate(() => window.layout.regions);
    assert.equal(new Set(regions.map(region => region.id)).size, regions.length);
    assert.deepEqual(await page.locator(".vr-player-display-panel").evaluate(panel => {
      const bounds = panel.getBoundingClientRect();
      return [...panel.querySelectorAll("button, .vr-menu-player-row")].filter(node => {
        const rect = node.getBoundingClientRect();
        return rect.left < bounds.left || rect.right > bounds.right || rect.top < bounds.top || rect.bottom > bounds.bottom;
      }).map(node => node.className);
    }), []);
    await page.screenshot({ path: join(screenshots, "wrist.png") });
    await page.evaluate(() => window.renderWrist(false));
    assert.equal(await page.locator('.vr-menu-processing [data-vr-menu]').first().isDisabled(), false, "Disabling filtering stays available on unsupported models");
    assert.ok(await page.locator('.vr-menu-player-row button').evaluateAll(nodes => nodes.every(node => node.disabled)));
    await page.evaluate(() => window.renderWrist(false, false));
    const presetWrist = page.locator('.vr-menu-processing [data-vr-menu]').first();
    assert.equal(await presetWrist.isDisabled(), false, "Wrist filtering can also be enabled before capture starts");
    assert.deepEqual(JSON.parse(await presetWrist.getAttribute("data-vr-menu")), { display: { set_microphone_filter: true } });
  } finally { await browser.close(); }
});
