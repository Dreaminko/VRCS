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

test("live translation scroll follows layout changes and respects user input", {
  timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
}, async t => {
  const bundle = await build({
    stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
      import React, { useState } from 'react';
      import { createRoot } from 'react-dom/client';
      import { flushSync } from 'react-dom';
      import i18next from 'i18next';
      import { initReactI18next } from 'react-i18next';
      import en from './src/i18n/locales/en-US.json';
      import { useLiveConversationScroll } from './src/conversations/useLiveConversationScroll';
      import { LiveView } from './src/subtitles/components/SubtitleViews';
      import { publishTranslationPartial, clearTranslationPartial, publishLivePartial, clearLivePartials } from './src/core-client/realtime-state';
      import './src/styles.css';
      await i18next.use(initReactI18next).init({ lng: 'en-US', resources: { 'en-US': { translation: en.translation } } });
      const subtitle = id => ({ id, text: 'Conversation message ' + id + ': Testing translation follow behavior.',
        language: 'en-US', source: 'speaker', created_at: '2026-10-19T00:00:00Z',
        started_at: null, ended_at: null, conversation_id: 'current', translations: [] });
      const initial = () => Array.from({ length: 18 }, (_, index) => subtitle(18 - index));
      let api, setRows, setStamp, setFocused;
      function App() {
        const [rows, updateRows] = useState(initial);
        const [stamp, updateStamp] = useState('0');
        const [focused, updateFocused] = useState(null);
        setRows = updateRows; setStamp = updateStamp; setFocused = updateFocused;
        api = useLiveConversationScroll({ page: 'live', activeConversationId: 'current', selectedConversationId: 'current',
          openedConversationId: 'current', loadingConversationSubtitles: false,
          selectedConversationUpdatedAt: stamp, focusedSubtitleId: focused });
        return <div className="app-shell live-shell sidebar-collapsed" style={{ height: 520 }}>
          <div className="window-chrome">Live scroll test</div>
          <div className="app-body"><div className="app-scroll-region" ref={api.liveScrollRef} onScroll={api.onLiveScroll}>
            <main className="workspace workspace-live">
              <input id="editor" aria-label="Editable text" />
              <LiveView subtitles={rows} scrollContainerRef={api.liveScrollRef} running={true} hasOlder={false}
                loading={false} loadingOlder={false} onLoadOlder={async () => {}} onSelect={async () => {}} />
            </main>
          </div></div>
        </div>;
      }
      const root = createRoot(document.getElementById('root'));
      flushSync(() => root.render(<App />));
      const region = () => document.querySelector('.app-scroll-region');
      const settle = () => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      const partial = text => flushSync(() => publishTranslationPartial(18, { text, target_language: 'zh-CN', preferred: true }));
      const state = () => ({ following: api.followingLiveSubtitles, top: region().scrollTop,
        gap: region().scrollHeight - region().clientHeight - region().scrollTop });
      window.fixture = {
        state, settle,
        async reset() {
          document.activeElement?.blur();
          clearLivePartials(); clearTranslationPartial(18);
          flushSync(() => { setRows(initial()); setStamp(String(performance.now())); setFocused(null); });
          api.scrollLiveViewToBottom('auto');
          await settle();
        },
        async shrinkAndGrow() {
          partial('Translation preview. '.repeat(150)); await settle();
          const longTop = region().scrollTop;
          partial('Final text.');
          const clampedTop = region().scrollTop;
          if (clampedTop >= longTop - 100) throw new Error('The fixture must clamp the real browser scroll offset');
          partial('Next translation. '.repeat(250));
          await settle();
        },
        async replaceLivePreview() {
          flushSync(() => publishLivePartial({ type: 'partial', source: 'speaker', utterance_id: 'next',
            text: 'Live recognition preview', language: 'en-US', translation: 'Temporary translation. '.repeat(150), target_language: 'zh-CN' }));
          await settle();
          const longTop = region().scrollTop;
          flushSync(() => clearLivePartials());
          const clampedTop = region().scrollTop;
          if (clampedTop >= longTop - 100) throw new Error('Clearing the live preview must clamp the offset');
          flushSync(() => { setRows(rows => [subtitle(19), ...rows]); setStamp(String(performance.now())); });
          partial('Saved translation. '.repeat(250));
          await settle();
        },
        async grow() { partial('More translation. '.repeat(350)); await settle(); },
        async bottom() { api.scrollLiveViewToBottom('auto'); await settle(); },
        async focusSubtitle() { flushSync(() => setFocused(10)); await settle(); },
        async clearFocus() { flushSync(() => setFocused(null)); await settle(); },
        focusScroll() { region().tabIndex = 0; region().focus({ preventScroll: true }); },
        bounds() {
          const rect = region().getBoundingClientRect();
          return { x: rect.left + 200, y: rect.top + 100, right: rect.right, bottom: rect.bottom };
        },
      };
    ` },
    plugins: [{ name: "locale-catalog-fixture", setup(build) {
      build.onResolve({ filter: /i18n\/catalog$/ }, () => ({ path: "locale-catalog", namespace: "fixture" }));
      build.onLoad({ filter: /.*/, namespace: "fixture" }, () => ({
        resolveDir: join(testDirectory, ".."),
        contents: 'import en from "./src/i18n/locales/en-US.json"; export const localeCatalog = [en]; export const supportedUiLocales = ["en-US"]; export const localeResources = { "en-US": { translation: en.translation } };',
      }));
    } }],
    bundle: true, write: false, outfile: "live-scroll-test.js", format: "esm", platform: "browser", jsx: "automatic",
    define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
  });
  const directory = mkdtempSync(join(testDirectory, ".live-scroll-browser-"));
  let browserProcess, socket;
  try {
    const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
    const stylesheet = bundle.outputFiles.find(file => file.path.endsWith(".css")).text;
    const html = join(directory, "test.html");
    writeFileSync(html, '<!doctype html><style>' + stylesheet
      + '</style><div id="root" style="width:900px"></div><script type="module">'
      + javascript.replace(/<\/script/gi, "<\\/script") + '</script>');
    browserProcess = spawn(browser, [
      "--headless=new", "--remote-debugging-port=0", "--disable-gpu", "--no-first-run",
      "--disable-background-networking", "--disable-extensions", "--window-size=1000,800",
      "--user-data-dir=" + join(directory, "profile"), pathToFileURL(html).href,
    ], { timeout: 40000 });
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
    socket.addEventListener("close", () => {
      for (const request of pending.values()) request.reject(new Error("Browser connection closed"));
      pending.clear();
    });
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
    const evaluate = async expression => {
      const response = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true }, sessionId);
      assert.equal(response.exceptionDetails, undefined, response.exceptionDetails?.exception?.description);
      return response.result?.value;
    };
    for (let attempt = 0; !await evaluate("Boolean(window.fixture)"); attempt++) {
      assert.ok(attempt < 100, "The live view fixture must load");
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    const input = (method, params) => send("Input." + method, params, sessionId);
    const state = () => evaluate("fixture.state()");
    const waitFor = async predicate => {
      for (let attempt = 0; attempt < 100; attempt++) {
        const current = await state();
        if (predicate(current)) return current;
        await new Promise(resolve => setTimeout(resolve, 20));
      }
      assert.fail("Scroll did not settle: " + JSON.stringify(await state()));
    };
    const followingBottom = async () => {
      const current = await state();
      assert.equal(current.following, true, JSON.stringify(current));
      assert.ok(Math.abs(current.gap) <= 1, JSON.stringify(current));
    };
    const settleInput = async () => {
      let previous = await state();
      let stable = 0;
      for (let attempt = 0; attempt < 100; attempt++) {
        await new Promise(resolve => setTimeout(resolve, 30));
        const current = await state();
        stable = Math.abs(current.top - previous.top) <= 1 ? stable + 1 : 0;
        if (stable >= 4) return;
        previous = current;
      }
      assert.fail("User scrolling did not settle");
    };
    const preservesHistory = async () => {
      await settleInput();
      const before = await state();
      assert.equal(before.following, false, JSON.stringify(before));
      await evaluate("fixture.grow()");
      const after = await state();
      assert.equal(after.following, false, JSON.stringify(after));
      assert.ok(Math.abs(after.top - before.top) <= 1, JSON.stringify({ before, after }));
    };

    for (const scenario of ["shrinkAndGrow", "replaceLivePreview"]) {
      await t.test("keeps following after " + scenario, async () => {
        await evaluate("fixture.reset()");
        await evaluate("fixture." + scenario + "()");
        await followingBottom();
        await evaluate("fixture.grow()");
        await followingBottom();
      });
    }
    await t.test("wheel scrolling pauses history and resumes at the bottom", async () => {
      await evaluate("fixture.reset()");
      const { x, y } = await evaluate("fixture.bounds()");
      const previous = await state();
      await input("dispatchMouseEvent", { type: "mouseWheel", x, y, deltaX: 0, deltaY: -240 });
      await waitFor(current => current.top < previous.top - 50 && !current.following);
      await preservesHistory();
      await input("dispatchMouseEvent", { type: "mouseWheel", x, y, deltaX: 0, deltaY: 10000 });
      await waitFor(current => current.following && Math.abs(current.gap) <= 1);
      await evaluate("fixture.grow()");
      await followingBottom();
    });
    await t.test("keyboard scrolling pauses history without intercepting editable fields", async () => {
      await evaluate("fixture.reset()");
      await evaluate("fixture.focusScroll()");
      await input("dispatchKeyEvent", { type: "keyDown", key: "PageUp", code: "PageUp", windowsVirtualKeyCode: 33 });
      await input("dispatchKeyEvent", { type: "keyUp", key: "PageUp", code: "PageUp", windowsVirtualKeyCode: 33 });
      await waitFor(current => !current.following && current.gap > 50);
      await preservesHistory();
      await evaluate("fixture.bottom()");
      await followingBottom();
      await evaluate("document.getElementById('editor').focus({ preventScroll: true })");
      await input("dispatchKeyEvent", { type: "keyDown", key: "ArrowUp", code: "ArrowUp", windowsVirtualKeyCode: 38 });
      await input("dispatchKeyEvent", { type: "keyUp", key: "ArrowUp", code: "ArrowUp", windowsVirtualKeyCode: 38 });
      await evaluate("fixture.shrinkAndGrow()");
      await followingBottom();
    });
    await t.test("scrollbar interaction can pause following", async () => {
      await evaluate("fixture.reset()");
      const bounds = await evaluate("fixture.bounds()");
      await input("dispatchMouseEvent", { type: "mousePressed", x: bounds.right - 7, y: bounds.y, button: "left", buttons: 1, clickCount: 1 });
      await waitFor(current => !current.following && current.gap > 50);
      await input("dispatchMouseEvent", { type: "mouseReleased", x: bounds.right - 7, y: bounds.y, button: "left", buttons: 0, clickCount: 1 });
      await preservesHistory();
    });
    await t.test("touch scrolling pauses following", async () => {
      await evaluate("fixture.reset()");
      await send("Emulation.setTouchEmulationEnabled", { enabled: true }, sessionId);
      try {
        const { x, y } = await evaluate("fixture.bounds()");
        await input("dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y }] });
        await input("dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x, y: y + 180 }] });
        await input("dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
        await waitFor(current => !current.following && current.gap > 50);
        await evaluate("fixture.settle()");
        await preservesHistory();
      } finally {
        await send("Emulation.setTouchEmulationEnabled", { enabled: false }, sessionId);
      }
    });
    await t.test("a focused subtitle stays paused until focus is cleared", async () => {
      await evaluate("fixture.reset()");
      await evaluate("fixture.focusSubtitle()");
      assert.equal((await state()).following, false);
      await evaluate("fixture.grow()");
      assert.equal((await state()).following, false);
      await evaluate("fixture.clearFocus()");
      await followingBottom();
    });
  } finally {
    socket?.close();
    if (browserProcess) {
      browserProcess.kill();
      await new Promise(resolve => browserProcess.exitCode !== null ? resolve() : browserProcess.once("exit", resolve));
    }
    rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
});
