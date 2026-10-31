import assert from "node:assert/strict";
import { spawn } from "node:child_process";
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

test("return to latest works after delayed desktop mount and remount", {
  timeout: 30000, skip: !browser && "Chrome or Edge is required",
}, async () => {
  const bundle = await build({
    stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
      import React, { StrictMode, useState } from 'react';
      import { createRoot } from 'react-dom/client';
      import { flushSync } from 'react-dom';
      import i18next from 'i18next';
      import { initReactI18next } from 'react-i18next';
      import en from './src/i18n/locales/en-US.json';
      import { DesktopShell } from './src/app/DesktopShell';
      import { useConversationWorkspace } from './src/conversations/useConversationWorkspace';
      import './src/styles.css';

      await i18next.use(initReactI18next).init({ lng: 'en-US', resources: { 'en-US': { translation: en.translation } } });
      const noop = () => {};
      const reportError = reason => { throw reason; };
      const conversation = { id: 'current', active: true, subtitle_count: 30,
        started_at: '2026-10-31T00:00:00Z', updated_at: '2026-10-31T00:00:00Z',
        ended_at: null, automatic_title: null, custom_title: null, icon: null };
      window.fetch = async () => new Response(JSON.stringify({ conversations: [conversation] }));
      const subtitle = id => ({ id, conversation_id: 'current', text: 'Message ' + id,
        source: 'speaker', language: 'en', created_at: conversation.started_at,
        started_at: null, ended_at: null, translations: [] });
      let showDesktop, appendSubtitle;
      function Fixture() {
        // The workspace survives while onboarding or compact mode replaces the desktop.
        const [shown, setShown] = useState(false);
        const [rows, setRows] = useState(() => Array.from({ length: 30 }, (_, i) => subtitle(30 - i)));
        showDesktop = value => flushSync(() => setShown(value));
        appendSubtitle = () => flushSync(() => setRows(current => [subtitle(current.length + 1), ...current]));
        const conversations = useConversationWorkspace({ coreReady: true, page: 'live',
          openedConversationId: 'current', subtitles: rows, conversationCatalogEvent: null,
          openConversation: noop, openConversationAt: noop, hasOlderSubtitles: false,
          reportError, clearErrorFrom: noop });
        return shown ? <DesktopShell page="live" settingsInitialCategory="system"
          setPage={noop} setSettingsInitialCategory={noop} workspace={{
            conversations, locale: 'en-US', runtime: { ready: true, connection: 'connected', health: null },
            capture: { playerAudioStart: {} }, compact: {}, settings: { value: null },
            subtitles: { openedConversationId: 'current', loadingConversation: false, focusedSubtitleId: null, loadOlder: noop },
            learning: { workspace: {}, actions: {} }, selection: { clear: noop, selectText: noop },
            providers: { catalog: { profiles: [], providerDefinitions: [] } }, chatbox: { open: false },
            integrations: { updater: { downloadedBytes: 0, totalBytes: null, noticeVisible: false } },
            dictionary: {}, onboarding: {}, playerDisplay: { defaultLanguages: [],
              preferences: { hiddenTracks: [], hiddenSourceLanguages: null },
              hiddenTracks: [], hiddenSourceLanguages: [], update: noop },
          }} /> : <p>Desktop hidden</p>;
      }
      const wait = async predicate => {
        for (let i = 0; i < 100; i++) {
          if (predicate()) return;
          await new Promise(resolve => setTimeout(resolve, 20));
        }
        throw new Error('Timed out: ' + predicate);
      };
      const check = (condition, message) => { if (!condition) throw new Error(message); };
      const settle = () => new Promise(resolve => setTimeout(resolve, 100));
      const region = () => document.querySelector('.app-scroll-region');
      const button = () => document.querySelector('.live-scroll-to-bottom');
      const atBottom = () => Math.abs(region().scrollHeight - region().clientHeight - region().scrollTop) <= 1;
      try {
        flushSync(() => createRoot(document.getElementById('root')).render(<StrictMode><Fixture /></StrictMode>));
        await settle();
        for (const phase of ['startup', 'desktop remount']) {
          showDesktop(true);
          await wait(() => region()?.scrollHeight - region()?.clientHeight > 1000);
          const scroll = region();
          scroll.scrollTop = scroll.scrollHeight;
          await settle();
          scroll.dispatchEvent(new WheelEvent('wheel', { deltaY: -240, bubbles: true }));
          scroll.scrollTop -= 240;
          await settle();
          check(button(), phase + ': scrolling up reveals the return button');
          check(button().getBoundingClientRect().height > 0, phase + ': the button is visible');
          const top = scroll.scrollTop;
          appendSubtitle();
          await settle();
          check(Math.abs(scroll.scrollTop - top) <= 1, phase + ': new messages preserve history position');
          flushSync(() => button().click());
          await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
          check(!atBottom(), phase + ': returning to latest animates instead of jumping');
          await wait(() => scroll.scrollTop > top);
          appendSubtitle();
          await wait(() => !button() && atBottom());
          appendSubtitle();
          await wait(atBottom);
          showDesktop(false);
          await settle();
        }
        document.getElementById('result').textContent = 'PASS';
      } catch (error) {
        document.getElementById('result').textContent = error.stack;
      }
    ` },
    plugins: [{ name: "locale-catalog-fixture", setup(build) {
      build.onResolve({ filter: /i18n\/catalog$/ }, () => ({ path: "locale-catalog", namespace: "fixture" }));
      build.onLoad({ filter: /.*/, namespace: "fixture" }, () => ({
        resolveDir: join(testDirectory, ".."),
        contents: 'import en from "./src/i18n/locales/en-US.json"; export const localeCatalog = [en]; export const supportedUiLocales = ["en-US"]; export const localeResources = { "en-US": { translation: en.translation } };',
      }));
    } }],
    bundle: true, write: false, outfile: "test.js", format: "esm", platform: "browser", jsx: "automatic",
    define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
  });
  const directory = mkdtempSync(join(testDirectory, ".live-scroll-lifecycle-browser-"));
  let browserProcess, socket;
  try {
    const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
    const stylesheet = bundle.outputFiles.find(file => file.path.endsWith(".css")).text;
    const html = join(directory, "test.html");
    writeFileSync(html, '<!doctype html><style>' + stylesheet + '</style><div id="root"></div><pre id="result">PENDING</pre><script type="module">'
      + javascript.replace(/<\/script/gi, "<\\/script") + '</script>');
    browserProcess = spawn(browser, [
      "--headless=new", "--disable-gpu", "--no-first-run", "--disable-background-networking", "--disable-extensions",
      "--window-size=1180,760", "--remote-debugging-port=0",
      "--user-data-dir=" + join(directory, "profile"), pathToFileURL(html).href,
    ], { timeout: 20000, windowsHide: true });
    const endpoint = await new Promise((resolve, reject) => {
      let stderr = "";
      browserProcess.once("error", reject);
      browserProcess.once("exit", code => reject(new Error("Browser exited: " + code + " " + stderr)));
      browserProcess.stderr.on("data", chunk => {
        stderr += chunk;
        const match = stderr.match(/DevTools listening on (ws:\/\/[^\s]+)/);
        if (match) resolve(match[1]);
      });
    });
    socket = new WebSocket(endpoint);
    await new Promise((resolve, reject) => {
      socket.addEventListener("open", resolve, { once: true });
      socket.addEventListener("error", reject, { once: true });
    });
    let id = 0;
    const pending = new Map();
    socket.addEventListener("message", event => {
      const response = JSON.parse(event.data);
      const request = pending.get(response.id);
      if (request) {
        pending.delete(response.id);
        response.error ? request.reject(new Error(response.error.message)) : request.resolve(response.result);
      }
    });
    const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
      const requestId = ++id;
      pending.set(requestId, { resolve, reject });
      socket.send(JSON.stringify({ id: requestId, method, params, sessionId }));
    });
    const { targetInfos } = await send("Target.getTargets");
    const page = targetInfos.find(target => target.type === "page");
    const { sessionId } = await send("Target.attachToTarget", { targetId: page.targetId, flatten: true });
    let outcome;
    for (let attempt = 0; attempt < 150; attempt++) {
      const response = await send("Runtime.evaluate", {
        expression: "document.getElementById('result')?.textContent", returnByValue: true,
      }, sessionId);
      outcome = response.result?.value;
      if (outcome && outcome !== "PENDING") break;
      await new Promise(resolve => setTimeout(resolve, 40));
    }
    assert.equal(outcome, "PASS");
    await send("Browser.close");
  } finally {
    socket?.close();
    if (browserProcess?.pid && browserProcess.exitCode === null) {
      browserProcess.kill();
      await new Promise(resolve => browserProcess.exitCode !== null ? resolve() : browserProcess.once("exit", resolve));
    }
    rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
});
