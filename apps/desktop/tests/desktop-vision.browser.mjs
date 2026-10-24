import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const { chromium } = createRequire(import.meta.url)("playwright");
const resolveDir = join(dirname(fileURLToPath(import.meta.url)), "..");

async function setup() {
  const bundle = await build({
    stdin: { resolveDir, loader: "tsx", contents: `
      import React, { useState } from 'react';
      import { createRoot } from 'react-dom/client';
      import i18next from 'i18next';
      import { initReactI18next } from 'react-i18next';
      import en from './src/i18n/locales/en-US.json';
      import { VisionWindow } from './src/vision/VisionWindow';
      import { VisionSettingsCard } from './src/settings/sections/VisionSettingsCard';
      import { DEFAULT_VISION_SETTINGS } from './src/settings/vision-settings';
      import { BottomDock } from './src/shell/BottomDock';
      import { openDesktopVision } from './src/vision/api';
      import './src/styles.css';
      await i18next.use(initReactI18next).init({ lng: 'en-US', resources: { 'en-US': { translation: en.translation } } });
      window.calls = [];
      window.order = [];
      window.created = [];
      window.revoked = [];
      window.pendingImages = {};
      window.status = { available: true, state: 'idle', session_id: 1, revision: 0, image_id: null,
        question: '', answer: '', error: null, error_detail: null, shortcut_error: null,
        shortcut: 'Ctrl+Alt+V', profile_name: 'Image service', model: 'image-model' };
      const png = Uint8Array.from(atob('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg=='), c => c.charCodeAt(0)).buffer;
      const create = URL.createObjectURL.bind(URL);
      const revoke = URL.revokeObjectURL.bind(URL);
      URL.createObjectURL = blob => { const url = create(blob); window.created.push({ url, type: blob.type }); return url; };
      URL.revokeObjectURL = url => { window.revoked.push(url); revoke(url); };
      Object.defineProperty(navigator, 'clipboard', { value: { writeText: async text => { window.copiedText = text; } }, configurable: true });
      window.fetch = async () => new Response(JSON.stringify({ models: ['image-model'] }));
      window.nativeInvoke = async (command, args) => {
        window.calls.push({ command, args });
        if (command === 'desktop_vision_status') {
          window.order.push('snapshot');
          return new Promise(resolve => { window.resolveSnapshot = resolve; });
        }
        if (command === 'desktop_vision_image') {
          if (window.deferImages) return new Promise(resolve => { window.pendingImages[args.imageId] = () => resolve(png); });
          return png;
        }
        if (command === 'desktop_vision_ask' && window.deferAsk) return new Promise((resolve, reject) => {
          window.resolveAsk = resolve;
          window.rejectAsk = reject;
        });
      };
      window.emit = patch => {
        window.status = { ...window.status, revision: window.status.revision + 1, ...patch };
        window.visionListener?.(window.status);
      };
      function SettingsHarness() {
        const [config, setConfig] = useState({ ...DEFAULT_VISION_SETTINGS, profile_id: 'images', model: 'image-model' });
        window.config = config;
        window.patchConfig = patch => setConfig(current => ({ ...current, ...patch }));
        return <VisionSettingsCard config={config} providerDefinitions={[]}
          recognition={{ backend: 'qwen_local_managed', active_profile_id: null, language: 'auto',
            managed_qwen: { package_id: 'qwen3-asr-0.6b-q8_0', device: 'auto' }, service_settings: {} }}
          profiles={[{ id: 'images', name: 'Image service', provider: 'openai',
          provider_display_name: 'OpenAI', enabled_capabilities: ['image_understanding'] }]}
          vrAvailable={false} disabled={false} onChange={window.patchConfig} onSelectProfile={() => {}} />;
      }
      const root = createRoot(document.getElementById('root'));
      window.mount = view => root.render(view === 'settings' ? <SettingsHarness /> : view === 'dock'
        ? <BottomDock learningAvailable={false} chatboxAvailable={false} page="live" running={false}
          chatboxOpen={false} captureDisabled={false} chatboxDisabled={false} chatboxButtonRef={null}
          onPageChange={() => {}} onCompact={() => {}} onChatbox={() => {}} onCapture={() => {}} onVision={openDesktopVision} />
        : <VisionWindow />);
      window.unmount = () => root.unmount();
      window.mount('vision');
    ` },
    plugins: [{ name: "native-contract", setup(build) {
      build.onResolve({ filter: /^@tauri-apps\/api\/(core|event)$/ }, args => ({ path: args.path, namespace: "mock-native" }));
      build.onLoad({ filter: /.*/, namespace: "mock-native" }, args => ({ contents: args.path.endsWith("/core")
        ? "export const isTauri = () => true; export const invoke = (command, args) => window.nativeInvoke(command, args);"
        : "export async function listen(name, callback) { window.order.push(name); const receive = payload => callback({ payload }); window.visionListener = receive; return () => { if (window.visionListener === receive) window.visionListener = null; }; }" }));
    } }],
    bundle: true, write: false, outfile: "vision-test.js", format: "esm", platform: "browser", jsx: "automatic",
    define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
  });
  const browser = await chromium.launch({ headless: true, ...(process.platform === "win32" ? { channel: "msedge" } : {}) });
  const page = await browser.newPage({ viewport: { width: 600, height: 780 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.setContent('<div id="root"></div>');
  await page.addStyleTag({ content: bundle.outputFiles.find(file => file.path.endsWith(".css")).text });
  await page.addScriptTag({ type: "module", content: bundle.outputFiles.find(file => file.path.endsWith(".js")).text });
  await page.waitForFunction(() => Boolean(window.resolveSnapshot));
  return { browser, page, errors };
}

async function emit(page, patch) {
  await page.evaluate(patch => window.emit(patch), patch);
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(resolve)));
}

async function commands(page, command) {
  return page.evaluate(command => window.calls.filter(call => call.command === command), command);
}

test("desktop VQA preserves edits, sends explicitly, and isolates voice and image lifecycles", { timeout: 60000 }, async () => {
  const { browser, page, errors } = await setup();
  try {
    assert.deepEqual(await page.evaluate(() => window.order), ["desktop-vision-status-changed", "snapshot"]);
    assert.deepEqual(await commands(page, "desktop_vision_capture"), []);
    await emit(page, { state: "idle" });
    await page.getByText(/Click Capture screen below/).waitFor();
    await emit(page, { state: "editing", image_id: 7, question: "Old recognition" });
    await page.evaluate(() => window.resolveSnapshot({ ...window.status, revision: 0, state: 'idle', image_id: null }));
    const question = page.getByRole("textbox", { name: "Question", exact: true });
    await question.waitFor();
    await page.waitForFunction(() => document.activeElement === document.querySelector('textarea'));
    const image = page.getByRole("img", { name: "Selected image to send" });
    await image.waitFor();
    await page.waitForFunction(() => document.querySelector('img')?.naturalWidth === 1);
    assert.match(await image.getAttribute("src"), /^blob:/);
    assert.deepEqual(await page.evaluate(() => window.created.map(item => item.type)), ["image/png"]);
    await question.fill("Typed question");
    await question.press("Shift+Enter");
    await question.press("A");
    assert.equal(await question.inputValue(), "Typed question\nA");
    await question.evaluate(element => {
      element.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
      element.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', isComposing: true, bubbles: true }));
      element.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', keyCode: 229, bubbles: true }));
      element.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }));
    });
    assert.equal((await commands(page, "desktop_vision_ask")).length, 0);
    await emit(page, { model: "new-model", question: "Old recognition", shortcut_error: "desktop_vision.shortcut_conflict" });
    assert.equal(await question.inputValue(), "Typed question\nA");
    assert.equal((await commands(page, "desktop_vision_image")).length, 1);
    await page.evaluate(() => { window.deferAsk = true; });
    await question.press("Enter");
    await page.waitForFunction(() => Boolean(window.rejectAsk));
    assert.deepEqual((await commands(page, "desktop_vision_ask"))[0].args, { question: "Typed question\nA", imageId: 7 });
    const send = page.getByRole("button", { name: "Send question", exact: true });
    assert.equal(await send.isDisabled(), true);
    await question.evaluate(element => element.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })));
    assert.equal((await commands(page, "desktop_vision_ask")).length, 1);
    // Cancel remains usable even when a command promise has not settled.
    await page.getByRole("button", { name: "Cancel current work" }).click();
    await page.evaluate(() => window.rejectAsk(new Error('Late failure from cancelled request')));
    await emit(page, { state: "error", error: "vision.request_failed", shortcut_error: null });
    assert.equal(await question.inputValue(), "Typed question\nA");
    assert.equal(await send.isEnabled(), true);
    assert.equal((await commands(page, "desktop_vision_image")).length, 1);
    await page.evaluate(() => { window.deferAsk = false; });
    await send.click();
    await emit(page, { state: "requesting", error: null });
    assert.equal(await send.isDisabled(), true);
    const answer = "Independent answer\n".repeat(100);
    await emit(page, { state: "complete", answer });
    await page.getByRole("button", { name: "Copy answer" }).click();
    assert.equal(await page.evaluate(() => window.copiedText), answer);
    assert.equal(await page.locator('.vision-window-content').evaluate(element => element.scrollHeight > element.clientHeight), true);
    assert.equal((await commands(page, "desktop_vision_start_recording")).length, 0);
    await page.getByRole("button", { name: "Ask another question" }).click();
    assert.equal(await question.evaluate(element => document.activeElement === element), true);
    await question.fill("Voice fallback");
    await page.getByRole("button", { name: "Start speaking" }).click();
    await emit(page, { state: "recording", question: "Old recognition" });
    await emit(page, { state: "recording", question: "Recognized question" });
    assert.equal(await question.inputValue(), "Recognized question");
    await page.getByRole("button", { name: "Finish recognition" }).click();
    await emit(page, { state: "finalizing_asr" });
    assert.equal(await send.isDisabled(), true);
    await emit(page, { state: "editing", question: "Final recognized question" });
    assert.equal(await question.inputValue(), "Final recognized question");
    assert.equal((await commands(page, "desktop_vision_ask")).length, 2, "finishing ASR must not send");
    await question.fill("Edited speech");
    await emit(page, { model: "other-model" });
    assert.equal(await question.inputValue(), "Edited speech");
    const before = await page.evaluate(() => window.calls.length);
    await page.getByRole("button", { name: "Select another area" }).click();
    assert.deepEqual(await page.evaluate(before => window.calls.slice(before).map(call => call.command), before), ["desktop_vision_cancel", "desktop_vision_capture"]);
    await emit(page, { state: "selecting" });
    assert.equal(await send.isDisabled(), true);
    await emit(page, { session_id: 2, revision: 0, state: "editing" });
    assert.equal(await question.inputValue(), "Edited speech");
    assert.equal((await commands(page, "desktop_vision_image")).length, 1, "cancelled selection retains the same preview");
    const oldUrl = await image.getAttribute("src");
    await emit(page, { image_id: 8, state: "editing" });
    await page.waitForFunction(oldUrl => document.querySelector('img')?.src !== oldUrl && window.revoked.includes(oldUrl), oldUrl);
    assert.deepEqual((await commands(page, "desktop_vision_image")).map(call => call.args), [{ imageId: 7 }, { imageId: 8 }]);
    await question.press("Escape");
    assert.equal((await commands(page, "desktop_vision_close")).length, 1);
    await emit(page, { state: "disabled", image_id: null, question: "", answer: "" });
    await question.waitFor({ state: "detached" });
    assert.equal(await page.evaluate(() => window.revoked.length), 2);
    await page.evaluate(() => window.unmount());
    assert.equal(await page.evaluate(() => window.visionListener), null);
    assert.deepEqual(errors, []);
  } finally { await browser.close(); }
});

test("late preview responses cannot replace a new image or leak object URLs after unmount", { timeout: 30000 }, async () => {
  const { browser, page, errors } = await setup();
  try {
    await page.evaluate(() => { window.deferImages = true; });
    await emit(page, { state: "editing", image_id: 7 });
    await page.waitForFunction(() => Boolean(window.pendingImages[7]));
    await emit(page, { image_id: 8 });
    await page.waitForFunction(() => Boolean(window.pendingImages[8]));
    await page.evaluate(() => window.pendingImages[8]());
    await page.getByRole("img").waitFor();
    const url = await page.getByRole("img").getAttribute("src");
    await page.evaluate(() => window.pendingImages[7]());
    await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 0)));
    assert.equal(await page.getByRole("img").getAttribute("src"), url);
    assert.equal(await page.evaluate(() => window.created.length), 1);
    await emit(page, { image_id: 9 });
    await page.waitForFunction(() => Boolean(window.pendingImages[9]));
    await page.evaluate(() => window.unmount());
    await page.evaluate(() => { window.pendingImages[9](); window.resolveSnapshot(window.status); });
    await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 0)));
    assert.equal(await page.evaluate(() => window.created.length), 1);
    assert.deepEqual(await page.evaluate(() => window.revoked), [url]);
    assert.deepEqual(errors, []);
  } finally { await browser.close(); }
});

test("common and desktop controls work without VR, show registration errors, and entries only open", { timeout: 30000 }, async () => {
  const { browser, page, errors } = await setup();
  try {
    await page.evaluate(() => window.mount('settings'));
    const master = page.getByRole("switch", { name: "Enable visual question answering", exact: true });
    const desktop = page.getByRole("switch", { name: "Enable desktop visual question answering", exact: true });
    const vr = page.getByRole("switch", { name: "Enable VR visual question answering", exact: true });
    await master.waitFor();
    assert.equal(await master.isEnabled(), true);
    assert.equal(await vr.count(), 0);
    await master.click();
    assert.equal(await vr.isDisabled(), true);
    assert.equal(await desktop.isEnabled(), true);
    await desktop.click();
    assert.equal(await page.evaluate(() => window.config.vr_enabled), true, "desktop edits preserve the VR preference");
    const shortcut = page.getByRole("button", { name: /^Screenshot shortcut/ });
    await shortcut.click();
    await shortcut.getByText("Press a shortcut…", { exact: true }).waitFor();
    await page.keyboard.press("Control+Shift+V");
    await page.waitForFunction(() => window.config.desktop_shortcut === 'Ctrl+Shift+V');
    await emit(page, { shortcut_error: "desktop_vision.shortcut_conflict" });
    await page.getByRole("alert").filter({ hasText: "This shortcut is already in use" }).waitFor();
    const open = page.getByRole("button", { name: "Open question window", exact: true });
    assert.equal(await open.isEnabled(), true);
    await open.click();
    assert.equal((await commands(page, "desktop_vision_open")).length, 1);
    assert.equal((await commands(page, "desktop_vision_capture")).length, 0);
    await master.click();
    assert.equal(await desktop.count(), 0);
    assert.equal(await page.evaluate(() => window.config.desktop_enabled), true, "master changes do not overwrite mode preferences");
    await page.evaluate(() => window.mount('dock'));
    await page.getByRole("button", { name: "Visual question answering", exact: true }).click();
    assert.equal((await commands(page, "desktop_vision_open")).length, 2);
    assert.equal((await commands(page, "desktop_vision_capture")).length, 0);
    assert.deepEqual(errors, []);
  } finally { await browser.close(); }
});
