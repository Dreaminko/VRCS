import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const testDirectory = dirname(fileURLToPath(import.meta.url));
const browser = [process.env.PROGRAMFILES, process.env["PROGRAMFILES(X86)"], process.env.LOCALAPPDATA]
  .filter(Boolean).flatMap((root) => [
    join(root, "Google/Chrome/Application/chrome.exe"),
    join(root, "Microsoft/Edge/Application/msedge.exe"),
  ]).find(existsSync);

test("conversation switching masks fast loads with smooth motion and shows skeleton only on slow loads", {
  timeout: 45000,
  skip: !browser && "Chrome or Edge is required",
}, async () => {
  const bundle = await build({
    stdin: {
      resolveDir: join(testDirectory, ".."),
      loader: "tsx",
      contents: `
        import React from 'react';
        import { createRoot } from 'react-dom/client';
        import { flushSync } from 'react-dom';
        import i18next from 'i18next';
        import { initReactI18next } from 'react-i18next';
        import en from './src/i18n/locales/en-US.json';
        import { DesktopShell } from './src/app/DesktopShell';
        import { useConversationHistory } from './src/conversations/useConversationHistory';
        import { useConversationWorkspace } from './src/conversations/useConversationWorkspace';
        import { initializeMotionPreference, updateReducedMotionPreference } from './src/app/app-motion';
        import './src/styles.css';

        await i18next.use(initReactI18next).init({
          lng: 'en-US',
          resources: { 'en-US': { translation: en.translation } },
        });
        const stopMotion = initializeMotionPreference();

        const conversationsList = ['conv-a', 'conv-b', 'conv-c', 'conv-d', 'conv-e', 'conv-f'].map((id, index) => ({
          id,
          active: id === 'conv-a',
          subtitle_count: 25,
          started_at: '2026-10-31T00:0' + index + ':00Z',
          updated_at: '2026-10-31T00:0' + index + ':00Z',
          ended_at: id === 'conv-a' ? null : '2026-10-31T00:09:00Z',
          automatic_title: 'Session ' + id.toUpperCase(),
          custom_title: null,
          icon: null,
        }));

        const makeSubtitles = (convId, count = 24) => Array.from({ length: count }, (_, idx) => {
          const id = count - idx;
          return {
            id: (convId.charCodeAt(5) * 100) + id,
            conversation_id: convId,
            text: convId + ' message ' + id,
            source: id % 2 === 0 ? 'microphone' : 'speaker',
            language: 'en',
            created_at: '2026-10-31T00:00:00Z',
            started_at: null,
            ended_at: null,
            translations: [],
          };
        });

        const delays = { 'conv-a': 10, 'conv-b': 20, 'conv-c': 320, 'conv-d': 320, 'conv-e': 25, 'conv-f': 20 };
        const fetchLog = [];
        const abortedIds = [];

        window.fetch = (input, init = {}) => {
          const url = String(input);
          if (url.endsWith('/api/conversations')) {
            return Promise.resolve(new Response(JSON.stringify({ conversations: conversationsList })));
          }
          const match = url.match(/\\/api\\/conversations\\/([^/]+)\\/subtitles/);
          if (!match) return Promise.resolve(new Response('{}'));
          const convId = decodeURIComponent(match[1]);
          fetchLog.push(convId);
          return new Promise((resolve, reject) => {
            const delay = delays[convId] ?? 20;
            const timer = setTimeout(() => {
              resolve(new Response(JSON.stringify({
                items: makeSubtitles(convId),
                has_more: false,
                next_before_id: null,
              })));
            }, delay);
            if (init.signal) {
              init.signal.addEventListener('abort', () => {
                clearTimeout(timer);
                abortedIds.push(convId);
                const err = new Error('Aborted');
                err.name = 'AbortError';
                reject(err);
              }, { once: true });
            }
          });
        };

        const noop = () => {};
        const reportError = (reason) => { throw reason; };

        function Harness() {
          const history = useConversationHistory({
            coreConfigured: true,
            reportError,
            clearErrorFrom: noop,
          });
          const conversations = useConversationWorkspace({
            coreReady: true,
            openedConversationId: history.openedConversationId,
            subtitles: history.subtitles,
            conversationCatalogEvent: null,
            openConversation: history.openConversation,
            openConversationAt: history.openConversationAt,
            page: 'live',
            hasOlderSubtitles: history.hasOlderSubtitles,
            reportError,
            clearErrorFrom: noop,
          });
          const selectedLoading = Boolean(
            history.loadingConversationSubtitles
            || (conversations.selectedConversation && conversations.selectedConversation.id !== history.openedConversationId)
          );
          return (
            <DesktopShell
              page="live"
              settingsInitialCategory="system"
              setPage={noop}
              setSettingsInitialCategory={noop}
              workspace={{
                conversations: { ...conversations, selectedLoading },
                locale: 'en-US',
                runtime: { ready: true, connection: 'connected', health: null, capabilities: {} },
                capture: { playerAudioStart: {} },
                compact: {},
                settings: { value: null },
                subtitles: {
                  openedConversationId: history.openedConversationId,
                  targetViewKey: history.targetViewKey,
                  committedViewKey: history.committedViewKey,
                  loadingConversation: history.loadingConversationSubtitles,
                  loadingOlder: history.loadingOlderSubtitles,
                  focusedSubtitleId: history.focusedSubtitleId,
                  translatingIds: history.translatingSubtitleIds,
                  loadOlder: history.loadOlderSubtitles,
                  openConversation: history.openConversation,
                },
                learning: { workspace: {}, actions: {} },
                selection: { clear: noop, selectText: noop },
                providers: { catalog: { profiles: [], providerDefinitions: [] } },
                chatbox: { open: false },
                integrations: { updater: { downloadedBytes: 0, totalBytes: null, noticeVisible: false } },
                dictionary: {},
                onboarding: {},
                playerDisplay: {
                  defaultLanguages: [],
                  preferences: { hiddenTracks: [], hiddenSourceLanguages: null },
                  hiddenTracks: [],
                  hiddenSourceLanguages: [],
                  update: noop,
                },
              }}
            />
          );
        }

        const check = (cond, msg) => { if (!cond) throw new Error(msg); };
        const settle = (ms = 20) => new Promise((r) => setTimeout(r, ms));
        const wait = async (pred, label = pred.toString()) => {
          for (let i = 0; i < 150; i++) {
            if (pred()) return;
            await settle(15);
          }
          throw new Error('Timed out: ' + label);
        };
        const stage = () => document.querySelector('.conversation-stage');
        const scrollRegion = () => document.querySelector('.app-scroll-region');
        const convButton = (id) => [...document.querySelectorAll('.conversation-button')]
          .find((el) => el.textContent?.includes('Session ' + id.toUpperCase()));

        try {
          flushSync(() => createRoot(document.getElementById('root')).render(<Harness />));
          await wait(() => document.body.textContent.includes('conv-a message 1') && stage()?.dataset.transitionPhase === 'idle', 'initial conv-a load');

          let sawSkeleton = false;
          let sawPhase = [];
          let sawDirections = [];
          const observer = new MutationObserver(() => {
            if (document.querySelector('.conversation-skeleton')) sawSkeleton = true;
            const p = stage()?.dataset.transitionPhase;
            if (p && sawPhase[sawPhase.length - 1] !== p) sawPhase.push(p);
            const d = stage()?.dataset.transitionDirection;
            if (d && sawDirections[sawDirections.length - 1] !== d) sawDirections.push(d);
          });
          observer.observe(stage(), {
            childList: true,
            subtree: true,
            attributes: true,
            attributeFilter: ['data-transition-phase', 'data-transition-direction'],
          });

          // 1. Fast switch A -> B (downward in sidebar, 20ms): exiting -> entering -> idle, never skeleton.
          convButton('conv-b').click();
          await wait(() => stage()?.dataset.transitionPhase === 'exiting', 'fast switch starts exiting');
          await wait(() => document.body.textContent.includes('conv-b message 1') && stage()?.dataset.transitionPhase === 'idle', 'fast switch settles on conv-b');
          check(!sawSkeleton, 'Fast switch never displays skeleton');
          check(sawPhase.includes('exiting') && sawPhase.includes('entering'), 'Fast switch goes through exiting and entering: ' + sawPhase.join(','));
          check(sawDirections.includes('down'), 'Downward switch uses down direction: ' + sawDirections.join(','));
          const region = scrollRegion();
          check(Math.abs(region.scrollHeight - region.clientHeight - region.scrollTop) <= 2, 'Switched conversation lands scrolled to bottom');

          // 2. Slow switch B -> C (320ms): exiting -> waiting -> skeleton -> entering -> idle.
          sawSkeleton = false;
          sawPhase = [];
          sawDirections = [];
          convButton('conv-c').click();
          await wait(() => stage()?.dataset.transitionPhase === 'waiting', 'slow switch enters waiting');
          check(!document.querySelector('.conversation-skeleton'), 'Skeleton is not shown immediately when entering waiting');
          await wait(() => stage()?.dataset.transitionPhase === 'skeleton' && Boolean(document.querySelector('.conversation-skeleton')), 'slow switch shows skeleton after ~200ms');
          await wait(() => document.body.textContent.includes('conv-c message 1') && stage()?.dataset.transitionPhase === 'idle', 'slow switch settles on conv-c');
          check(sawPhase.includes('waiting') && sawPhase.includes('skeleton') && sawPhase.includes('entering'), 'Slow switch phases: ' + sawPhase.join(','));
          check(sawDirections.includes('down'), 'Slow downward switch uses down direction: ' + sawDirections.join(','));

          // 3. Rapid switch C -> D (320ms) -> E (25ms): aborts D and settles on E.
          sawSkeleton = false;
          convButton('conv-d').click();
          await settle(25);
          convButton('conv-e').click();
          await wait(() => document.body.textContent.includes('conv-e message 1') && stage()?.dataset.transitionPhase === 'idle', 'rapid switch settles on conv-e');
          check(abortedIds.includes('conv-d'), 'Rapid switch aborts in-flight conv-d request');
          check(!sawSkeleton, 'Rapid switch finishing on fast conv-e does not show skeleton');

          // 4. Cache revisit E -> B (upward in sidebar, even with slow background revalidation): up direction, no skeleton.
          delays['conv-b'] = 350;
          sawSkeleton = false;
          sawDirections = [];
          convButton('conv-b').click();
          await wait(() => document.body.textContent.includes('conv-b message 1') && stage()?.dataset.transitionPhase === 'idle', 'cached revisit settles on conv-b');
          check(!sawSkeleton, 'Cached revisit never shows skeleton even when background revalidation is slow');
          check(sawDirections.includes('up'), 'Upward cached revisit uses up direction: ' + sawDirections.join(','));

          // 5. Reduced motion B -> F (20ms): skips exiting/entering and does not flash skeleton.
          updateReducedMotionPreference(true);
          sawSkeleton = false;
          sawPhase = [];
          convButton('conv-f').click();
          await wait(() => document.body.textContent.includes('conv-f message 1') && stage()?.dataset.transitionPhase === 'idle', 'reduced motion settles on conv-f');
          check(!sawSkeleton, 'Reduced motion fast switch never shows skeleton');
          check(!sawPhase.includes('exiting') && !sawPhase.includes('entering'), 'Reduced motion skips exiting/entering phases: ' + sawPhase.join(','));

          observer.disconnect();
          updateReducedMotionPreference(false);
          stopMotion();
          document.getElementById('result').textContent = 'PASS';
        } catch (error) {
          document.getElementById('result').textContent = 'FAIL: ' + (error?.stack || error?.message || String(error));
        }
      `,
    },
    plugins: [{
      name: "locale-catalog-fixture",
      setup(buildPlugin) {
        buildPlugin.onResolve({ filter: /i18n\/catalog$/ }, () => ({ path: "locale-catalog", namespace: "fixture" }));
        buildPlugin.onLoad({ filter: /.*/, namespace: "fixture" }, () => ({
          resolveDir: join(testDirectory, ".."),
          contents: 'import en from "./src/i18n/locales/en-US.json"; export const localeCatalog = [en]; export const supportedUiLocales = ["en-US"]; export const localeResources = { "en-US": { translation: en.translation } };',
        }));
      },
    }],
    bundle: true,
    write: false,
    outfile: "conversation-switch-motion-test.js",
    format: "esm",
    platform: "browser",
    jsx: "automatic",
    define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
  });

  const directory = mkdtempSync(join(testDirectory, ".conversation-switch-motion-"));
  let browserProcess, socket, send, server;
  try {
    const javascript = bundle.outputFiles.find((file) => file.path.endsWith(".js")).text;
    const stylesheet = bundle.outputFiles.find((file) => file.path.endsWith(".css")).text;
    const html = '<!doctype html><link rel="stylesheet" href="/test.css"><div id="root"></div><pre id="result">PENDING</pre>'
      + '<script>window.addEventListener("error", e => document.getElementById("result").textContent = "ERR: " + e.message); window.addEventListener("unhandledrejection", e => document.getElementById("result").textContent = "REJ: " + String(e.reason));</script>'
      + '<script type="module" src="/test.js"></script>';
    server = createServer((req, res) => {
      if (req.url === "/test.js") {
        res.writeHead(200, { "Content-Type": "application/javascript; charset=utf-8" });
        res.end(javascript);
      } else if (req.url === "/test.css") {
        res.writeHead(200, { "Content-Type": "text/css; charset=utf-8" });
        res.end(stylesheet);
      } else {
        res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
        res.end(html);
      }
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const { port } = server.address();
    const testUrl = `http://127.0.0.1:${port}/`;
    browserProcess = spawn(browser, [
      "--headless=new", "--disable-gpu", "--disable-crash-reporter", "--no-first-run", "--disable-background-networking", "--disable-extensions",
      "--window-size=1180,760", "--remote-debugging-port=0",
      "--user-data-dir=" + join(directory, "profile"), "about:blank",
    ], { timeout: 30000, windowsHide: true });
    const endpoint = await new Promise((resolve, reject) => {
      let stderr = "";
      browserProcess.once("error", reject);
      browserProcess.once("exit", (code) => reject(new Error("Browser exited: " + code + " " + stderr)));
      browserProcess.stderr.on("data", (chunk) => {
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
    socket.addEventListener("message", (event) => {
      const response = JSON.parse(event.data);
      const request = pending.get(response.id);
      if (request) {
        pending.delete(response.id);
        response.error ? request.reject(new Error(response.error.message)) : request.resolve(response.result);
      }
    });
    const sendCdp = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
      const requestId = ++id;
      pending.set(requestId, { resolve, reject });
      socket.send(JSON.stringify({ id: requestId, method, params, sessionId }));
    });
    send = sendCdp;
    const { targetInfos } = await send("Target.getTargets");
    const page = targetInfos.find((target) => target.type === "page");
    const { sessionId } = await send("Target.attachToTarget", { targetId: page.targetId, flatten: true });
    await send("Runtime.enable", {}, sessionId);
    await send("Page.enable", {}, sessionId);
    const errors = [];
    socket.addEventListener("message", (event) => {
      const msg = JSON.parse(event.data);
      if (msg.method === "Runtime.exceptionThrown") {
        errors.push(JSON.stringify(msg.params.exceptionDetails));
      } else if (msg.method === "Runtime.consoleAPICalled" && msg.params.type === "error") {
        errors.push(msg.params.args.map((a) => a.value ?? a.description).join(" "));
      }
    });
    await send("Page.navigate", { url: testUrl }, sessionId);
    let outcome;
    for (let attempt = 0; attempt < 250; attempt++) {
      const response = await send("Runtime.evaluate", {
        expression: "document.getElementById('result')?.textContent",
        returnByValue: true,
      }, sessionId);
      outcome = response.result?.value;
      if (outcome && outcome !== "PENDING") break;
      await new Promise((resolve) => setTimeout(resolve, 60));
    }
    assert.equal(outcome, "PASS", errors.join("\n"));
  } finally {
    if (socket?.readyState === WebSocket.OPEN && send) {
      await send("Browser.close").catch(() => undefined);
      socket.close();
    } else {
      socket?.close();
    }
    server?.close();
    if (browserProcess?.pid && browserProcess.exitCode === null) {
      const exited = new Promise((resolve) => browserProcess.once("exit", resolve));
      const timeout = setTimeout(() => {
        if (browserProcess.exitCode === null) browserProcess.kill();
      }, 2000);
      await exited;
      clearTimeout(timeout);
    }
    try {
      rmSync(directory, { recursive: true, force: true, maxRetries: 15, retryDelay: 200 });
    } catch {
      // Ignore transient Windows file lock on headless browser profile cleanup.
    }
  }
});
