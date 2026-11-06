import assert from "node:assert/strict";
import { execFile, spawn } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { promisify } from "node:util";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const testDirectory = dirname(fileURLToPath(import.meta.url));
const browser = [process.env.PROGRAMFILES, process.env["PROGRAMFILES(X86)"], process.env.LOCALAPPDATA]
  .filter(Boolean).flatMap(root => [
    join(root, "Google/Chrome/Application/chrome.exe"),
    join(root, "Microsoft/Edge/Application/msedge.exe"),
  ]).find(existsSync);

for (const [width, pointerType] of [[1200, "mouse"], [380, "touch"]]) {
  test(`settings tabs reorder after a long press with ${pointerType} at ${width}px`, {
    timeout: 45000, skip: !browser && "Chrome or Edge is required",
  }, async () => {
    const bundle = await build({
      stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
        import React, { useState } from 'react';
        import { createRoot } from 'react-dom/client';
        import { flushSync } from 'react-dom';
        import i18next from 'i18next';
        import { initReactI18next } from 'react-i18next';
        import en from './src/i18n/locales/en-US.json';
        import { SettingsTabBar } from './src/settings/components/SettingsTabBar';
        import { applyInterfaceScale } from './src/app/interface-scale';
        import './src/styles.css';
        await i18next.use(initReactI18next).init({ lng: 'en-US', resources: { 'en-US': { translation: en.translation } } });
        window.requestAnimationFrame = callback => setTimeout(() => callback(performance.now()), 16);
        window.cancelAnimationFrame = id => clearTimeout(id);
        const defaults = ['system', 'audio', 'recognition', 'translation', 'vr_overlay', 'vision', 'glossary', 'api', 'learning', 'connections', 'about'];
        localStorage.clear();
        let remount, setVisible;
        function Harness() {
          const [active, setActive] = useState('system');
          const [key, setKey] = useState(0);
          const [visible, updateVisible] = useState(defaults);
          remount = () => setKey(value => value + 1);
          setVisible = updateVisible;
          return <SettingsTabBar key={key} activeCategory={active} visibleCategories={visible} onChange={setActive} />;
        }
        const root = createRoot(document.getElementById('root'));
        flushSync(() => root.render(<Harness />));
        const check = (condition, message) => { if (!condition) throw new Error(message); };
        const settle = (ms = 30) => new Promise(resolve => setTimeout(resolve, ms));
        const tab = id => document.getElementById('settings-tab-' + id);
        const ids = () => [...document.querySelectorAll('[role=tab]')].map(element => element.id.replace('settings-tab-', ''));
        const editing = () => document.querySelector('.settings-tabbar-reordering');
        const active = () => document.querySelector('[role=tab][aria-selected=true]').id;
        const click = element => flushSync(() => element.click());
        const key = (element, value, extra = {}) => flushSync(() => element.dispatchEvent(new KeyboardEvent('keydown', { key: value, bubbles: true, cancelable: true, ...extra })));
        const pointer = (element, type, x, y, extra = {}) => flushSync(() => element.dispatchEvent(new PointerEvent(type, {
          bubbles: true, cancelable: true, pointerId: 7, isPrimary: true, pointerType: '${pointerType}',
          button: 0, buttons: type === 'pointerup' || type === 'pointercancel' ? 0 : 1, clientX: x, clientY: y, ...extra,
        })));
        const center = element => { const rect = element.getBoundingClientRect(); return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }; };
        const press = async id => {
          const element = tab(id), point = center(element);
          pointer(element, 'pointerdown', point.x, point.y);
          await settle(560);
          check(editing(), 'A long press enters reorder mode');
          return { element, ...point };
        };
        const outside = () => pointer(document.getElementById('outside'), 'pointerdown', 10, 160);
        try {
          check(ids().join() === defaults.join(), 'Default category order is preserved');
          click(tab('audio'));
          check(active() === 'settings-tab-audio' && !editing(), 'A normal click still changes the category');
          key(tab('audio'), 'End');
          check(active() === 'settings-tab-about' && document.activeElement === tab('about'), 'Normal keyboard navigation still works');
          key(tab('about'), 'Home');
          for (const value of ['ArrowLeft', 'ArrowRight']) key(tab('system'), value, { altKey: true });
          check(!editing() && ids().join() === defaults.join() && active() === 'settings-tab-system', 'Alt plus arrow keys cannot start reordering or move tabs');
          await settle(180);
          let point = center(tab('audio'));
          pointer(tab('audio'), 'pointerdown', point.x, point.y);
          pointer(tab('audio'), 'pointerup', point.x, point.y);
          await settle(560);
          check(!editing(), 'A short press does not arm reorder mode');
          pointer(tab('audio'), 'pointerdown', point.x, point.y);
          pointer(tab('audio'), 'pointermove', point.x + 16, point.y);
          await settle(560);
          check(!editing(), 'Moving before the long press cancels activation');
          pointer(tab('audio'), 'pointerup', point.x + 16, point.y);

          const held = await press('audio');
          check(document.querySelectorAll('.settings-tabbar-container button').length === defaults.length, 'Reorder mode adds no Done button');
          check(!document.querySelector('.settings-tab-tooltip'), 'Dragging suppresses tab tooltips');
          const target = center(tab('translation'));
          pointer(held.element, 'pointermove', target.x + 5, target.y);
          await settle(80);
          check(ids().indexOf('audio') === 3, 'The held icon moves across adjacent icons');
          pointer(held.element, 'pointerup', target.x + 5, target.y);
          click(held.element);
          check(active() === 'settings-tab-system', 'Releasing a dragged icon does not switch settings pages');
          check(editing(), 'Reorder mode stays open after a drop');
          const saved = ids().join();
          pointer(document.querySelector('.settings-tabbar-wrap'), 'pointerdown', 10, held.y);
          check(!editing(), 'Clicking empty space beside the tab bar exits reorder mode');
          flushSync(remount);
          check(ids().join() === saved, 'Remounting restores the saved order');

          const cancelled = await press('recognition');
          const first = center(tab(ids()[0]));
          pointer(cancelled.element, 'pointermove', first.x, first.y);
          await settle(60);
          check(ids().join() !== saved, 'Dragging previews a new order');
          key(cancelled.element, 'Escape');
          check(!editing() && ids().join() === saved, 'Escape cancels an unfinished drag and exits');
          pointer(cancelled.element, 'pointerup', first.x, first.y);
          flushSync(remount);
          check(ids().join() === saved, 'A cancelled drag is not persisted');

          const interrupted = await press('audio');
          pointer(interrupted.element, 'pointermove', first.x, first.y);
          await settle(60);
          pointer(interrupted.element, 'pointercancel', first.x, first.y);
          check(ids().join() === saved, 'Pointer cancellation restores the order');
          const next = await press('audio');
          const start = center(tab(ids()[0]));
          pointer(next.element, 'pointermove', start.x - 8, start.y);
          await settle(60);
          pointer(next.element, 'pointerup', start.x - 8, start.y);
          const pointerSaved = ids().join();
          check(ids()[0] === 'audio', 'Dragging can move an icon to the first position');
          tab('audio').focus();
          for (const value of ['ArrowLeft', 'ArrowRight', 'Home', 'End']) {
            key(tab('audio'), value);
            key(tab('audio'), value, { altKey: true });
          }
          check(ids().join() === pointerSaved && active() === 'settings-tab-system' && editing(), 'Navigation keys do not move tabs or change pages in reorder mode');
          key(tab('audio'), 'Escape');
          check(!editing(), 'Escape exits reorder mode after a drop');
          flushSync(remount);
          check(ids().join() === pointerSaved, 'Only the dragged order persists');

          flushSync(() => setVisible(defaults.filter(id => id !== 'vision')));
          check(!tab('vision'), 'Disabled feature tabs stay hidden');
          const hidden = await press('recognition');
          const last = center(tab('about'));
          pointer(hidden.element, 'pointermove', last.x + 5, last.y);
          await settle(80);
          pointer(hidden.element, 'pointerup', last.x + 5, last.y);
          outside();
          flushSync(() => setVisible(defaults));
          const restored = ids();
          check(restored.length === defaults.length && new Set(restored).size === defaults.length && restored.at(-1) === 'recognition', 'Hidden categories retain their place without duplicate or missing tabs');

          if ('${pointerType}' === 'touch') {
            const scroller = document.querySelector('.settings-tabbar-wrap');
            scroller.scrollLeft = 0;
            const edge = await press('audio');
            const bounds = scroller.getBoundingClientRect();
            pointer(edge.element, 'pointermove', bounds.right - 4, edge.y);
            await settle(900);
            check(scroller.scrollLeft > 60, 'Dragging near an edge scrolls the overflowing tab bar');
            pointer(edge.element, 'pointerup', bounds.right - 4, edge.y);
            outside();
            check(document.documentElement.scrollWidth <= innerWidth + 1, 'Reordering keeps the page inside the narrow viewport');
          }
          applyInterfaceScale(150);
          await settle(180);
          const scroller = document.querySelector('.settings-tabbar-wrap');
          scroller.scrollLeft = 0;
          const scaled = await press(ids()[1]);
          const before = center(scaled.element);
          pointer(scaled.element, 'pointermove', before.x + 18, before.y);
          await settle(40);
          check(Math.abs(center(scaled.element).x - before.x - 18) < 3, 'The dragged icon follows the pointer at 150 percent interface scale');
          key(scaled.element, 'Escape');
          applyInterfaceScale(100);

          const pending = center(tab(ids()[1]));
          pointer(tab(ids()[1]), 'pointerdown', pending.x, pending.y);
          flushSync(remount);
          await settle(560);
          check(!editing(), 'Unmounting cancels pending long presses');
          localStorage.setItem('vrcs.settingsTabOrder', JSON.stringify(['about', 'about', 'unknown', 42, 'audio']));
          flushSync(remount);
          check(ids()[0] === 'about' && ids()[1] === 'audio' && ids().length === defaults.length && new Set(ids()).size === defaults.length, 'Saved orders discard duplicates and unknown tabs and append missing categories');
          localStorage.setItem('vrcs.settingsTabOrder', '{invalid');
          flushSync(remount);
          check(ids().join() === defaults.join(), 'Invalid stored data falls back to the default order');
          const quick = await press('audio');
          const destination = center(tab('translation'));
          pointer(quick.element, 'pointermove', destination.x + 5, destination.y);
          pointer(quick.element, 'pointerup', destination.x + 5, destination.y);
          check(ids().indexOf('audio') === 3, 'A quick drop uses the final pointer position before the next animation frame');
          document.getElementById('result').textContent = 'PASS';
        } catch (error) {
          document.getElementById('result').textContent = 'FAIL: ' + error.message;
        }
      ` },
      bundle: true, write: false, outfile: "settings-tab-order.js", format: "esm", platform: "browser", jsx: "automatic",
    });
    const directory = mkdtempSync(join(testDirectory, ".settings-tab-order-browser-"));
    try {
      const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
      const css = bundle.outputFiles.find(file => file.path.endsWith(".css")).text;
      writeFileSync(join(directory, "content.html"), '<!doctype html><style>body{margin:0}</style><style>' + css
        + '</style><div class="settings-surface" id="root"></div><div id="outside" style="height:200px">Outside</div><pre id="result">PENDING</pre>'
        + '<script>const report=document.getElementById("result");window.addEventListener("error",e=>report.textContent="FAIL: "+e.message);window.addEventListener("unhandledrejection",e=>report.textContent="FAIL: "+e.reason);new MutationObserver(()=>parent.postMessage(report.textContent,"*")).observe(report,{childList:true});</script>'
        + '<script type="module">' + javascript.replace(/<\/script/gi, '<\\/script') + '</script>');
      const html = join(directory, "test.html");
      writeFileSync(html, `<iframe src="content.html" style="display:block;border:0;width:${width}px;height:700px"></iframe><pre id="result">PENDING</pre>`
        + '<script>window.addEventListener("message",e=>document.getElementById("result").textContent=e.data);</script>');
      const { stdout, stderr } = await promisify(execFile)(browser, [
        "--headless=new", "--dump-dom", "--virtual-time-budget=12000", "--disable-gpu", "--no-first-run",
        "--disable-background-networking", "--disable-extensions", `--window-size=${width},800`,
        "--user-data-dir=" + join(directory, "profile"), pathToFileURL(html).href,
      ], { windowsHide: true, timeout: 35000, maxBuffer: 8 * 1024 * 1024 });
      const report = stdout.match(/<pre id="result">([^<]*)<\/pre>/)?.[1];
      assert.equal(report, "PASS", report || stderr);
    } finally {
      assert.equal(dirname(directory), testDirectory);
      rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
    }
  });
}

test("native tab reordering eases between positions, retains capture, and honors reduced motion", {
  timeout: 45000, skip: !browser && "Chrome or Edge is required",
}, async () => {
  const bundle = await build({
    stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
      import React, { useState } from 'react';
      import { createRoot } from 'react-dom/client';
      import i18next from 'i18next';
      import { initReactI18next } from 'react-i18next';
      import zh from './src/i18n/locales/zh-CN.json';
      import { SettingsTabBar } from './src/settings/components/SettingsTabBar';
      import { initializeMotionPreference, updateReducedMotionPreference } from './src/app/app-motion';
      import './src/styles.css';
      await i18next.use(initReactI18next).init({ lng: 'zh-CN', resources: { 'zh-CN': { translation: zh.translation } } });
      initializeMotionPreference();
      function Harness() {
        const [active, setActive] = useState('system');
        return <SettingsTabBar activeCategory={active} onChange={setActive}
          visibleCategories={['system', 'audio', 'recognition', 'translation', 'vr_overlay', 'vision', 'glossary', 'api', 'learning', 'connections', 'about']} />;
      }
      createRoot(document.getElementById('root')).render(<Harness />);
      window.fixture = {
        setReducedMotion: updateReducedMotionPreference,
        point: id => { const rect = document.getElementById('settings-tab-' + id).getBoundingClientRect(); return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }; },
        sampleMotion: id => {
          const tab = document.getElementById('settings-tab-' + id);
          const animation = tab.getAnimations().find(animation => animation.constructor.name === 'Animation'
            && animation.effect.getKeyframes().some(frame => frame.transform));
          if (!animation) return null;
          animation.pause();
          const duration = Number(animation.effect.getTiming().duration);
          animation.currentTime = 0;
          const start = tab.getBoundingClientRect().left;
          animation.currentTime = duration / 4;
          const quarter = tab.getBoundingClientRect().left;
          animation.finish();
          const end = tab.getBoundingClientRect().left;
          return { start, quarter, end, duration };
        },
        positionAnimations: () => [...document.querySelectorAll('[role=tab]')].flatMap(tab => tab.getAnimations())
          .filter(animation => animation.constructor.name === 'Animation' && animation.effect.getKeyframes().some(frame => frame.transform)).length,
        state: () => ({ ids: [...document.querySelectorAll('[role=tab]')].map(tab => tab.dataset.settingsCategory),
          editing: Boolean(document.querySelector('.settings-tabbar-reordering')), dragging: Boolean(document.querySelector('.settings-tab-dragging')),
          active: document.querySelector('[aria-selected=true]')?.dataset.settingsCategory,
          saved: JSON.parse(localStorage.getItem('vrcs.settingsTabOrder') ?? 'null') }),
      };
    ` },
    bundle: true, write: false, outfile: "native-tabs.js", format: "esm", platform: "browser", jsx: "automatic",
  });
  const directory = mkdtempSync(join(testDirectory, ".settings-tab-order-browser-native-"));
  let browserProcess;
  let socket;
  try {
    const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
    const css = bundle.outputFiles.find(file => file.path.endsWith(".css")).text;
    const html = join(directory, "test.html");
    writeFileSync(html, '<!doctype html><style>' + css + '</style><div class="settings-surface" id="root"></div>'
      + '<script type="module">' + javascript.replace(/<\/script/gi, '<\\/script') + '</script>');
    browserProcess = spawn(browser, ["--headless=new", "--remote-debugging-port=0", "--disable-gpu", "--no-first-run",
      "--disable-background-networking", "--disable-extensions", "--window-size=1200,800",
      "--user-data-dir=" + join(directory, "profile"), pathToFileURL(html).href,
    ], { timeout: 35000, windowsHide: true });
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
      const response = JSON.parse(event.data), request = pending.get(response.id);
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
      const result = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true }, sessionId);
      assert.equal(result.exceptionDetails, undefined, result.exceptionDetails?.exception?.description);
      return result.result?.value;
    };
    const settle = ms => new Promise(resolve => setTimeout(resolve, ms));
    for (let attempt = 0; !await evaluate("Boolean(document.querySelector('[role=tab]'))"); attempt++) {
      assert.ok(attempt < 100, "The tab fixture must load");
      await settle(25);
    }
    const input = (method, params) => send("Input." + method, params, sessionId);
    for (const [pointerType, appReduced, systemReduced] of [["mouse", false, false], ["touch", false, false], ["mouse", true, false], ["touch", false, true]]) {
      const reduced = appReduced || systemReduced;
      await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-motion", value: systemReduced ? "reduce" : "no-preference" }] }, sessionId);
      await evaluate(`fixture.setReducedMotion(${appReduced})`);
      await send("Emulation.setTouchEmulationEnabled", { enabled: pointerType === "touch" }, sessionId);
      const start = await evaluate("fixture.point('audio')");
      if (pointerType === "mouse") {
        await input("dispatchMouseEvent", { type: "mouseMoved", ...start });
        await input("dispatchMouseEvent", { type: "mousePressed", ...start, button: "left", buttons: 1, clickCount: 1 });
      } else await input("dispatchTouchEvent", { type: "touchStart", touchPoints: [{ ...start, id: 1 }] });
      await settle(560);
      assert.equal((await evaluate("fixture.state()")).editing, true, pointerType + ": long press enters reorder mode");
      const target = await evaluate("fixture.point('glossary')");
      target.x += 8;
      if (pointerType === "mouse") await input("dispatchMouseEvent", { type: "mouseMoved", ...target, button: "left", buttons: 1 });
      else await input("dispatchTouchEvent", { type: "touchMove", touchPoints: [{ ...target, id: 1 }] });
      await settle(80);
      const preview = await evaluate("fixture.state()");
      assert.equal(preview.ids[6], "audio", pointerType + ": drag crosses several icons without losing capture: " + JSON.stringify(preview));
      assert.equal(preview.dragging, true, pointerType + ": drag remains active after DOM reordering");
      if (!reduced) {
        const motion = await evaluate("fixture.sampleMotion('recognition')");
        assert.ok(motion && Math.abs(motion.end - motion.start) > 20, "Neighboring icons animate between their old and new positions");
        const progress = (motion.quarter - motion.start) / (motion.end - motion.start);
        assert.ok(progress > 0.4 && progress < 1, "Neighbor movement eases out instead of moving at a constant speed: " + JSON.stringify(motion));
      } else assert.equal(await evaluate("fixture.positionAnimations()"), 0, "Reduced motion skips animated reordering");
      await settle(250);
      assert.equal((await evaluate("fixture.state()")).ids[6], "audio", "Animated neighbors do not change the insertion target while the pointer is stationary");
      if (process.env.VRCS_TAB_SCREENSHOT && pointerType === "mouse") {
        const screenshot = await send("Page.captureScreenshot", { format: "png" }, sessionId);
        writeFileSync(process.env.VRCS_TAB_SCREENSHOT, Buffer.from(screenshot.data, "base64"));
      }
      if (pointerType === "mouse") await input("dispatchMouseEvent", { type: "mouseReleased", ...target, button: "left", buttons: 0, clickCount: 1 });
      else await input("dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
      await settle(50);
      if (!reduced) {
        const motion = await evaluate("fixture.sampleMotion('audio')");
        assert.ok(motion && Math.abs(motion.end - motion.start) > 1, "The released icon animates from the pointer position into its slot");
        const progress = (motion.quarter - motion.start) / (motion.end - motion.start);
        assert.ok(progress > 0.4 && progress < 1, "Drop motion eases out without overshooting: " + JSON.stringify(motion));
      } else assert.equal(await evaluate("fixture.positionAnimations()"), 0, "Reduced motion skips animated drops");
      const result = await evaluate("fixture.state()");
      assert.equal(result.active, "system", pointerType + ": release does not change the active category");
      assert.deepEqual(result.saved, result.ids, pointerType + ": a native drop persists the new order");
      await input("dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
      await input("dispatchKeyEvent", { type: "keyUp", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
      assert.equal((await evaluate("fixture.state()")).editing, false, "Escape exits after a native drop");
      await evaluate("localStorage.clear(); location.reload()");
      await settle(200);
    }
  } finally {
    socket?.close();
    if (browserProcess) {
      browserProcess.kill();
      await new Promise(resolve => browserProcess.exitCode !== null ? resolve() : browserProcess.once("exit", resolve));
    }
    assert.equal(dirname(directory), testDirectory);
    rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
});
