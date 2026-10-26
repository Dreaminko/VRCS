import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const testDirectory = dirname(fileURLToPath(import.meta.url));
const browser = [process.env.PROGRAMFILES, process.env["PROGRAMFILES(X86)"], process.env.LOCALAPPDATA]
  .filter(Boolean).flatMap(root => [
    join(root, "Google/Chrome/Application/chrome.exe"),
    join(root, "Microsoft/Edge/Application/msedge.exe"),
  ]).find(existsSync);

for (const [width, reduced] of [[1100, false], [860, false], [380, false], [1100, true]]) {
  test(`the whole status bar toggles to a single button at ${width}px (reduced: ${reduced})`, {
    timeout: 45000, skip: !browser && "Chrome or Edge is required",
  }, async () => {
    const bundle = await build({
      stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
        import React, {useRef, useState} from 'react';
        import {createRoot} from 'react-dom/client';
        import {flushSync} from 'react-dom';
        import i18next from 'i18next';
        import {initReactI18next} from 'react-i18next';
        import zh from './src/i18n/locales/zh-CN.json';
        import en from './src/i18n/locales/en-US.json';
        import {TopStatus} from './src/subtitles/components/TopStatus';
        import {PlayerHistoryControls} from './src/subtitles/components/PlayerHistoryControls';
        import {WorkspaceFrame} from './src/app/components/WorkspaceFrame';
        import {updateReducedMotionPreference} from './src/app/app-motion';
        import './src/styles.css';

        await i18next.use(initReactI18next).init({lng: 'zh-CN', resources: {
          'zh-CN': {translation: zh.translation}, 'en-US': {translation: en.translation},
        }});
        localStorage.clear();
        let reconnect, remount;
        function Fixture() {
          const scroll = useRef(null);
          const [connection, setConnection] = useState('connected');
          const [version, setVersion] = useState(0);
          reconnect = setConnection;
          remount = () => setVersion(value => value + 1);
          return <div className="app-shell live-shell sidebar-collapsed">
            <div className="window-chrome">VRCS</div><div className="app-body">
              <WorkspaceFrame page="live" scrollRef={scroll} onScroll={() => {}} toolbar={
                <TopStatus key={version} connection={connection} health={null} settings={null} controls={
                  <PlayerHistoryControls subtitles={[]} enabled={false} selectedUserIds={null}
                    defaultLanguages={[]} preferences={{hiddenTracks: [], hiddenSourceLanguages: null}}
                    onPreferences={() => {}} onSelection={async () => {}} />
                } />
              }><p>Conversation</p></WorkspaceFrame>
            </div>
          </div>;
        }
        createRoot(document.getElementById('root')).render(<Fixture />);
        const check = (value, message) => { if (!value) throw Error(message); };
        const wait = async predicate => {
          for (let i = 0; i < 100; i++) {
            if (predicate()) return;
            await new Promise(resolve => setTimeout(resolve, 20));
          }
          throw Error('Timed out: ' + predicate);
        };
        const click = element => flushSync(() => element.click());
        const pointerClick = element => flushSync(() => element.dispatchEvent(new MouseEvent('click', {bubbles: true, detail: 1})));
        const motion = () => document.getAnimations().filter(animation => animation.constructor.name === 'Animation');
        const finishMotion = async () => {
          for (const animation of document.getAnimations()) animation.finish();
          await wait(() => !motion().length);
        };
        const statusButton = () => document.querySelector('.top-status-row > button');
        const personnel = () => document.querySelector('.player-history-trigger');
        const paintedBounds = surface => {
          const bounds = surface.getBoundingClientRect();
          const outline = surface.querySelector('.status-motion-outline');
          if (!outline) return bounds;
          const clip = getComputedStyle(outline).clipPath.slice(6).split(' round ')[0].split(' ').map(parseFloat);
          const [top, right = top, bottom = top, left = right] = clip;
          const zoom = bounds.width / parseFloat(getComputedStyle(surface).width);
          return new DOMRect(bounds.left + left * zoom, bounds.top + top * zoom,
            bounds.width - (left + right) * zoom, bounds.height - (top + bottom) * zoom);
        };
        const checkBounds = button => {
          const bounds = button.getBoundingClientRect();
          check(bounds.width > 0 && bounds.height > 0, 'the status control remains visible');
          check(bounds.left >= 0 && bounds.right <= innerWidth, 'status stays within the window');
          check(personnel().getBoundingClientRect().right < bounds.left, 'personnel and status do not overlap');
          check(document.documentElement.scrollWidth <= innerWidth, 'the toolbar does not widen the page');
        };
        try {
          await wait(() => document.querySelector('.top-status-row'));
          const button = statusButton();
          check(button?.classList.contains('status-summary'), 'the entire expanded status bar is a native button');
          check(button.getAttribute('aria-expanded') === 'true', 'status starts expanded');
          check(button.querySelectorAll('button').length === 0, 'there is no separate collapse button');
          check(button.textContent.includes('Core') && button.textContent.includes('Qwen3-ASR 0.6B'), 'the existing status information is retained');
          check(button.getAttribute('aria-label') === '收起 应用状态', 'expanded status announces its collapse action');
          const initialText = button.textContent;
          checkBounds(button);
          check(!motion().length, 'initial rendering does not animate');
          const expandedRect = button.getBoundingClientRect();
          const personnelRect = personnel().getBoundingClientRect();
          pointerClick(button);
          check(button.getAttribute('aria-expanded') === 'false', 'collapse responds before its animation finishes');
          if (!${reduced}) {
            const animations = motion();
            check(animations.length > 0, 'pointer collapse has a transition');
            for (const animation of animations) { animation.pause(); animation.currentTime = 0; }
            const surface = button.querySelector('.status-motion-surface');
            check(surface, 'the background animates separately from the text');
            const start = paintedBounds(surface);
            check(Math.abs(start.width - expandedRect.width) < 1 && Math.abs(start.right - expandedRect.right) < 1, 'collapse starts at the visible expanded shape');
            check(Math.abs(personnel().getBoundingClientRect().left - personnelRect.left) < 1, 'the personnel button does not jump when collapse starts');
            for (const animation of animations) animation.currentTime = animation.effect.getTiming().duration / 2;
            const middle = paintedBounds(surface);
            check(middle.width > 42 && middle.width < start.width, 'collapse passes through intermediate sizes');
            check(Math.abs(middle.width - (start.width + 42) / 2) > 2, 'collapse uses a non-linear curve');
            check(Math.abs(middle.right - start.right) < 1, 'the right edge remains anchored');
            check(middle.left > start.left, 'the left edge closes toward the fixed right edge');
            const surfaceTransform = new DOMMatrix(getComputedStyle(surface).transform);
            check(surfaceTransform.a === 1 && surfaceTransform.d === 1, 'the rounded outline and shadow are never stretched');
            const roundCorners = element => {
              const corners = getComputedStyle(element).clipPath.split(' round ')[1];
              check(corners && !corners.includes('/'), 'the outline retains circular corner arcs, not elliptical radii');
              return parseFloat(corners);
            };
            const middleRadius = roundCorners(surface.querySelector('.status-motion-outline'));
            check(Math.abs(middleRadius - roundCorners(surface.querySelector('.status-motion-fill')) - 1) < 0.1, 'the outline keeps its one-pixel thickness during motion');
            const movingPersonnel = personnel().getBoundingClientRect();
            pointerClick(button);
            for (const animation of motion()) { animation.pause(); animation.currentTime = 0; }
            check(Math.abs(paintedBounds(surface).width - middle.width) < 1, 'reversing mid-animation preserves the visible size');
            check(Math.abs(roundCorners(surface.querySelector('.status-motion-outline')) - middleRadius) < 0.1, 'reversing mid-animation preserves the visible corner radius');
            check(Math.abs(personnel().getBoundingClientRect().left - movingPersonnel.left) < 1, 'reversing mid-animation preserves the personnel position');
            for (const animation of motion()) animation.currentTime = animation.effect.getTiming().duration / 2;
            check(paintedBounds(surface).width > middle.width + (expandedRect.width - middle.width) * 0.65, 'expansion decelerates toward its final size');
            const contentTransform = new DOMMatrix(getComputedStyle(button.querySelector('.status-motion-content')).transform);
            check(contentTransform.a === 1 && contentTransform.d === 1, 'status text is never stretched');
          } else check(!motion().length, 'system reduced motion skips the transition');
          await finishMotion();
          if (button.getAttribute('aria-expanded') === 'false') click(button);
          checkBounds(button);
          if (!${reduced}) {
            pointerClick(button);
            updateReducedMotionPreference(true);
            await finishMotion();
            pointerClick(button);
            check(!motion().length, 'the app reduced-motion setting also skips transitions');
            updateReducedMotionPreference(false);
            document.body.style.zoom = '1.25';
            pointerClick(button);
            for (const animation of motion()) { animation.pause(); animation.currentTime = 0; }
            check(Math.abs(button.querySelector('.status-motion-surface').getBoundingClientRect().right - button.getBoundingClientRect().right) < 1, 'motion remains anchored when interface scale changes');
            await finishMotion();
            click(button);
            document.body.style.zoom = '';
          }
          button.focus();
          click(button.querySelector('strong'));
          check(!motion().length, 'keyboard activation does not delay the toggle');
          check(statusButton() === button && button.getAttribute('aria-expanded') === 'false', 'clicking status content collapses the same button');
          check(document.activeElement === button, 'collapsing preserves keyboard focus');
          check(!button.textContent.trim() && button.querySelector('svg'), 'collapsed status is one icon-only button');
          const a = button.getBoundingClientRect(), b = personnel().getBoundingClientRect();
          check(a.width === b.width && a.height === b.height && getComputedStyle(button).borderRadius === '50%', 'the collapsed button matches the personnel circle');
          const painted = paintedBounds(button.querySelector('.status-motion-surface'));
          check(painted.width === 42 && painted.height === 42, 'the settled outline is a 42px circle');
          check(button.getAttribute('aria-label') === '展开 应用状态', 'collapsed status announces its expand action');
          checkBounds(button);
          for (const theme of ['light', 'dark']) {
            document.documentElement.dataset.theme = theme;
            await finishMotion();
            check(getComputedStyle(button.querySelector('.status-motion-fill') ?? button).backgroundColor === getComputedStyle(personnel()).backgroundColor, 'both buttons share theme surfaces');
          }
          click(button);
          check(button.textContent === initialText && button.getAttribute('aria-expanded') === 'true', 'one click restores all status details');
          click(button);
          flushSync(remount);
          check(statusButton().getAttribute('aria-expanded') === 'false', 'collapse preference survives remounting');
          flushSync(() => reconnect('disconnected'));
          click(statusButton());
          check(statusButton().textContent.includes('未连接'), 'reopening shows current connection state');
          flushSync(remount);
          check(statusButton().getAttribute('aria-expanded') === 'true', 'expanded preference also survives remounting');
          await i18next.changeLanguage('en-US');
          await wait(() => statusButton().textContent.includes('Disconnected'));
          check(statusButton().getAttribute('aria-label') === 'Collapse Application status', 'toggle labels follow the selected language');
          checkBounds(statusButton());
          click(statusButton());
          click(personnel());
          await wait(() => document.querySelector('.player-history-panel'));
          await finishMotion();
          check(statusButton().getAttribute('aria-expanded') === 'false', 'opening personnel does not expand status');
          flushSync(() => document.dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', bubbles: true})));
          await finishMotion();
          await wait(() => !document.querySelector('.player-history-panel'));
          const getItem = Storage.prototype.getItem, setItem = Storage.prototype.setItem;
          try {
            Storage.prototype.getItem = Storage.prototype.setItem = () => { throw Error('Storage unavailable'); };
            flushSync(remount);
            check(statusButton().getAttribute('aria-expanded') === 'true', 'unavailable storage falls back to expanded status');
            click(statusButton());
            check(statusButton().getAttribute('aria-expanded') === 'false', 'storage errors do not prevent toggling');
          } finally {
            Storage.prototype.getItem = getItem;
            Storage.prototype.setItem = setItem;
          }
          const frame = ${JSON.stringify(process.env.VRCS_STATUS_MOTION_FRAME ?? "")};
          if (frame && !${reduced}) {
            document.documentElement.dataset.theme = 'light';
            await i18next.changeLanguage('zh-CN');
            const startExpanded = frame === 'collapse' || frame === 'expanded';
            if ((statusButton().getAttribute('aria-expanded') === 'true') !== startExpanded) click(statusButton());
            if (frame === 'collapse' || frame === 'expand') {
              pointerClick(statusButton());
              for (const animation of motion()) {
                animation.pause();
                animation.currentTime = animation.effect.getTiming().duration / 2;
              }
            }
          }
          document.getElementById('result').textContent = 'PASS';
        } catch (error) { document.getElementById('result').textContent = 'FAIL: ' + error.message; }
      ` },
      bundle: true, write: false, outfile: "top-status-test.js", format: "esm", platform: "browser", jsx: "automatic",
      define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
    });
    const directory = mkdtempSync(join(testDirectory, ".top-status-browser-"));
    try {
      const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
      const css = bundle.outputFiles.find(file => file.path.endsWith(".css")).text;
      writeFileSync(join(directory, "content.html"), '<style>' + css + '</style><div id="root"></div><pre id="result" hidden>PENDING</pre>'
        + '<script>const result = document.getElementById("result"); window.addEventListener("error", e => result.textContent = "FAIL: " + e.message); window.addEventListener("unhandledrejection", e => result.textContent = "FAIL: " + e.reason); new MutationObserver(() => parent.postMessage(result.textContent, "*")).observe(result, {childList:true});</script>'
        + '<script type="module">' + javascript.replace(/<\/script/gi, "<\\/script") + '</script>');
      const html = join(directory, "index.html");
      writeFileSync(html, '<iframe src="content.html" style="border:0;width:' + width + 'px;height:620px"></iframe><pre id="result">PENDING</pre><script>window.addEventListener("message", e => document.getElementById("result").textContent = e.data);</script>');
      const result = spawnSync(browser, [
        "--headless=new", "--dump-dom", "--virtual-time-budget=8000", "--disable-gpu", "--no-first-run",
        "--disable-background-networking", "--disable-extensions",
        ...(reduced ? ["--force-prefers-reduced-motion=reduce"] : []),
        ...(process.env.VRCS_STATUS_SCREENSHOT ? ["--screenshot=" + process.env.VRCS_STATUS_SCREENSHOT + "-" + width + ".png"] : []),
        "--window-size=1200,800", "--user-data-dir=" + join(directory, "profile"), pathToFileURL(html).href,
      ], { encoding: "utf8", timeout: 30000, maxBuffer: 8 * 1024 * 1024, windowsHide: true });
      assert.ifError(result.error);
      assert.equal(result.status, 0, result.stderr);
      const report = result.stdout.match(/<pre id="result">([^<]*)<\/pre>/)?.[1];
      assert.equal(report, "PASS", report || result.stderr);
    } finally {
      assert(relative(testDirectory, directory).startsWith(".top-status-browser-"));
      rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
    }
  });
}
