// Run with an installed Chrome or Edge: node --test tests/control-tooltip.browser.mjs
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
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

test("compact controls use themed tooltips instead of native titles", {
  timeout: 60000, skip: !browser && "Chrome or Edge is required",
}, async () => {
  const bundle = await build({
    stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
      import React, {useState} from 'react';
      import {createRoot} from 'react-dom/client';
      import i18next from 'i18next';
      import {initReactI18next} from 'react-i18next';
      import locale from './src/i18n/locales/zh-CN.json';
      import {CompactView} from './src/shell/CompactView';
      import {COMPACT_LAYOUT,COMPACT_FONT_PRESETS} from './src/shell/compact-layout';
      import {WindowChrome} from './src/shell/WindowChrome';
      import {ControlTooltip} from './src/shared/ui/ControlTooltip';
      import {applyInterfaceScale, INTERFACE_LAYOUT_CHANGE_EVENT} from './src/app/interface-scale';
      import './src/styles.css';
      await i18next.use(initReactI18next).init({lng:'zh-CN',resources:{'zh-CN':{translation:locale.translation}}});
      const preset=COMPACT_FONT_PRESETS.medium;
      const layoutVariables={...Object.fromEntries(Object.entries(COMPACT_LAYOUT).map(([name,value])=>[
        '--compact-'+name.replace(/[A-Z]/g,letter=>'-'+letter.toLowerCase()),value+'px'])),
        '--compact-original-size':preset.original+'px','--compact-original-line':preset.originalLine+'px',
        '--compact-translation-size':preset.translation+'px','--compact-translation-line':preset.translationLine+'px'};
      let setTransparent, setRunning, setMounted;
      function Fixture(){
        const [transparent,updateTransparent]=useState(false), [running,updateRunning]=useState(false), [mounted,updateMounted]=useState(true);
        setTransparent=updateTransparent;setRunning=updateRunning;setMounted=updateMounted;
        return <><div className={'compact-root'+(transparent?' compact-root-transparent':'')} style={layoutVariables}>
        {mounted && <CompactView transparentBackground={transparent} onToggleBackground={()=>updateTransparent(value=>!value)} backgroundOpacity={75}
          onBackgroundOpacityChange={()=>{}} fontSize="medium" onFontSizeChange={()=>{}}
          subtitles={[]} subtitleHistory={[]} selectionTarget={null} running={running} vrchatMuted={false}
          captureDisabled={false} onSelect={async()=>{}} onCapture={()=>updateRunning(value=>!value)} onRestore={()=>{}} onClose={()=>{}} onResize={()=>{}} />}
        </div><div hidden><WindowChrome /></div><ControlTooltip />
        <button className="dock-button" data-tooltip="Existing dock hint" hidden>Dock</button></>;
      }
      createRoot(document.getElementById('root')).render(<Fixture />);
      const check=(condition,message)=>{if(!condition)throw new Error(message)};
      const wait=async predicate=>{for(let i=0;i<100;i++){if(predicate())return;await new Promise(resolve=>setTimeout(resolve,20))}throw new Error('Timed out')};
      const tick=()=>new Promise(resolve=>setTimeout(resolve,30));
      const tooltip=()=>document.querySelector('.control-tooltip');
      const over=(element,pointerType='mouse')=>element.dispatchEvent(new PointerEvent('pointerover',{bubbles:true,pointerType}));
      const leave=element=>element.dispatchEvent(new PointerEvent('pointerout',{bubbles:true,pointerType:'mouse'}));
      const bounds=()=>{const element=tooltip(),rect=element.getBoundingClientRect();check(rect.left>=4&&rect.right<=innerWidth-4&&rect.top>=4&&rect.bottom<=innerHeight-4,'Tooltip stays within the small viewport: '+JSON.stringify({rect:rect.toJSON(),viewport:[innerWidth,innerHeight],offset:[element.offsetWidth,element.offsetHeight],style:element.style.cssText,width:getComputedStyle(element).width,font:getComputedStyle(element).font}))};
      const hover=async element=>{over(element);await wait(()=>tooltip()?.textContent===element.dataset.tooltip);bounds()};
      try {
        await wait(()=>document.querySelector('.compact-window-actions button'));
        await tick();
        check(!document.querySelector('#root [title]'),'Compact and window controls must not expose native title tooltips');
        const controls=[...document.querySelectorAll('.compact-root [data-tooltip]')];
        check(controls.length===8,'All compact controls, status and resize grip have custom hints');
        for(const element of controls){
          over(element);await tick();check(!tooltip(),'Pointer tooltips wait before showing');
          await wait(()=>tooltip()?.textContent===element.dataset.tooltip);bounds();
          check(tooltip().parentElement===document.body,'Tooltips escape compact overflow clipping');
          check(element.getAttribute('aria-describedby')?.includes(tooltip().id),'Visible hint describes its control');
          leave(element);await wait(()=>!tooltip());
          check(!element.hasAttribute('aria-describedby'),'Closing removes the temporary description');
        }
        const restore=document.querySelector('.compact-window-actions > button:not(.compact-close-button)');
        restore.setAttribute('aria-describedby','existing-description');
        await hover(restore);leave(restore);await wait(()=>!tooltip());
        check(restore.getAttribute('aria-describedby')==='existing-description','Existing accessible descriptions survive');
        over(restore,'touch');await new Promise(resolve=>setTimeout(resolve,220));
        check(!tooltip(),'Touch does not open a hover hint');
        restore.focus();await wait(()=>tooltip());
        check(tooltip().classList.contains('keyboard'),'Keyboard focus shows a hint immediately');
        document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));
        await wait(()=>!tooltip());restore.blur();
        await hover(restore);restore.click();await wait(()=>!tooltip());
        restore.disabled=true;
        await hover(restore);leave(restore);await wait(()=>!tooltip());
        restore.disabled=false;restore.setAttribute('aria-expanded','true');
        await hover(restore);leave(restore);await wait(()=>!tooltip());
        restore.removeAttribute('aria-expanded');
        const font=document.querySelector('.compact-font-button');
        await hover(font);font.click();await wait(()=>document.querySelector('.compact-font-menu'));
        over(font);await new Promise(resolve=>setTimeout(resolve,220));
        check(!tooltip(),'Open menus suppress their trigger hints');
        document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));
        await wait(()=>!document.querySelector('.compact-font-menu'));font.blur();
        const capture=document.querySelector('.compact-capture-button');
        await hover(capture);const oldLabel=tooltip().textContent;setRunning(true);
        await wait(()=>tooltip()?.textContent===capture.dataset.tooltip&&tooltip().textContent!==oldLabel);
        leave(capture);await wait(()=>!tooltip());
        document.documentElement.dataset.theme='dark';setTransparent(true);
        await wait(()=>!document.querySelector('.compact-opacity-control'));
        for(const scale of [75,150]){
          await applyInterfaceScale(scale);
          const close=document.querySelector('.compact-close-button');
          await hover(close);
          check(getComputedStyle(tooltip()).backgroundColor==='rgb(47, 52, 62)','Hints use the current dark theme');
          leave(close);await wait(()=>!tooltip());
          await hover(document.querySelector('.compact-resize-grip'));
          check(tooltip().dataset.side==='top','Bottom-edge hints flip above the control');
          leave(document.querySelector('.compact-resize-grip'));await wait(()=>!tooltip());
        }
        const dock=document.querySelector('.dock-button');over(dock);
        await new Promise(resolve=>setTimeout(resolve,220));check(!tooltip(),'Existing dock hints are not duplicated');
        await hover(restore);window.dispatchEvent(new Event(INTERFACE_LAYOUT_CHANGE_EVENT));await wait(()=>!tooltip());
        await hover(restore);document.dispatchEvent(new Event('scroll'));await wait(()=>!tooltip());
        await hover(restore);window.dispatchEvent(new Event('blur'));await wait(()=>!tooltip());
        await hover(restore);setMounted(false);await wait(()=>!tooltip());
        setMounted(true);await wait(()=>document.querySelector('.compact-font-button'));
        await applyInterfaceScale(100);await hover(document.querySelector('.compact-font-button'));
        for(const animation of tooltip().getAnimations()) animation.finish();
        check(getComputedStyle(tooltip()).opacity==='1'&&getComputedStyle(tooltip()).visibility==='visible','Hints are visible after the reveal animation');
        document.getElementById('result').textContent='PASS';
      } catch(error) {document.getElementById('result').textContent='FAIL: '+error.message}
    ` },
    bundle: true, write: false, outfile: "control-tooltip-test.js", format: "esm", platform: "browser", jsx: "automatic",
    define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
  });
  const directory = mkdtempSync(join(tmpdir(), "vrcs-control-tooltip-"));
  try {
    const html = join(directory, "index.html");
    const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
    const stylesheet = bundle.outputFiles.find(file => file.path.endsWith(".css"))?.text ?? "";
    writeFileSync(html, '<style>' + stylesheet + '</style><div id="root"></div><pre id="result" hidden>PENDING</pre>'
      + '<script type="module">' + javascript.replace(/<\/script/gi, "<\\/script") + '</script>');
    const result = spawnSync(browser, [
      "--headless=new", "--dump-dom", "--virtual-time-budget=12000", "--disable-gpu", "--no-first-run",
      "--disable-background-networking", "--disable-extensions", "--disable-software-rasterizer", "--no-sandbox",
      ...(process.env.VRCS_TOOLTIP_SCREENSHOT ? ["--screenshot=" + process.env.VRCS_TOOLTIP_SCREENSHOT] : []),
      "--window-size=500,300", "--user-data-dir=" + join(directory, "profile"), pathToFileURL(html).href,
    ], { encoding: "utf8", timeout: 30000, maxBuffer: 8 * 1024 * 1024 });
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    const report = result.stdout.match(/<pre id="result"[^>]*>([^<]*)<\/pre>/)?.[1];
    assert.equal(report, "PASS", `${report || "No result"}\n${result.stderr}\n${result.stdout.slice(-2000)}`);
  } finally {
    rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
});
