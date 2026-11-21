import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const testDirectory = dirname(fileURLToPath(import.meta.url));
const browser = [process.env.PROGRAMFILES, process.env["PROGRAMFILES(X86)"], process.env.LOCALAPPDATA]
  .filter(Boolean).flatMap(root => [join(root, "Google/Chrome/Application/chrome.exe"), join(root, "Microsoft/Edge/Application/msedge.exe")]).find(existsSync);

test("Chatbox capture prepares before start and hot save without restarting other audio", {
  timeout: 45000, skip: !browser && "Chrome or Edge is required",
}, async () => {
  const bundle = await build({
    stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
      import React from 'react';
      import {createRoot} from 'react-dom/client';
      import i18next from 'i18next';
      import {initReactI18next} from 'react-i18next';
      import en from './src/i18n/locales/en-US.json';
      import {useSettingsRuntime} from './src/settings/hooks/useSettingsRuntime';
      import {useCaptureControl} from './src/core-client/useCaptureControl';
      import {usesVrchatNetwork} from './src/capture/vrchat-network';
      import {createControlExecutor} from './src/vr-controls/actions';
      await i18next.use(initReactI18next).init({lng:'en-US',resources:{'en-US':{translation:en.translation}}});
      const testing=window.testing={events:[],config:{features:{},vrchat_chatbox:{enabled:false},
        audio:{output:{mode:'system',device_id:null,player_audio:{enabled:true}},microphone:{mode:'disabled',device_id:null}}},running:true};
      let confirmed=true, privileges='ready', rejectSave=false, runtime, capture;
      const noop=()=>{};
      const setRunning=async target=>{testing.events.push(target?'start':'stop');testing.running=target;return true;};
      const health={getCurrent:()=>({capture_requested:testing.running}),refreshQuietly:async()=>{}};
      const gate={confirm:async()=>{testing.events.push('confirm');return confirmed;},
        ensurePrivileges:async()=>{testing.events.push('privileges');if(privileges==='cancelled')throw Error('UAC cancelled');return privileges==='ready';},
        prepare:async()=>{testing.events.push('prepare');await new Promise(resolve=>setTimeout(resolve,20));return confirmed;}};
      function Fixture(){
        runtime=useSettingsRuntime({active:false,coreConfigured:true,audioSupported:true,health,stopMicrophoneTest:async()=>{},setCaptureRunning:setRunning,
          playerAudioStart:gate,clearErrorFrom:noop,reportError:noop});
        capture=useCaptureControl({coreReady:true,getRunning:()=>testing.running,outputMode:runtime.value?.audio.output.mode,
          networkCaptureEnabled:usesVrchatNetwork(runtime.value),preparePlayerAudioStart:gate.prepare,
          createConversation:async()=>true,clearLookup:noop,setCoreCaptureRunning:setRunning,clearErrorFrom:noop,reportError:noop});
        return null;
      }
      createRoot(document.getElementById('root')).render(<Fixture/>);
      const check=(ok,message)=>{if(!ok)throw Error(message);};
      const wait=async predicate=>{for(let i=0;i<150;i++){if(predicate())return;await new Promise(r=>setTimeout(r,15));}throw Error('Timed out: '+predicate);};
      const enable=value=>runtime.update(current=>({...current,vrchat_chatbox:{enabled:value}}));
      const events=expected=>check(testing.events.join('|')===expected,'Expected '+expected+'; got '+testing.events.join('|'));
      try{
        await wait(()=>runtime?.value);
        confirmed=false;await enable(true);events('confirm');
        check(!runtime.getCurrent().vrchat_chatbox.enabled&&testing.running,'declining leaves old settings and audio running');
        confirmed=true;privileges='cancelled';testing.events=[];
        await enable(true).then(()=>{throw Error('cancelled UAC unexpectedly succeeded');},()=>{});events('confirm|privileges');
        check(!runtime.getCurrent().vrchat_chatbox.enabled&&testing.running,'UAC cancellation cannot enable the consumer');
        privileges='relaunching';testing.events=[];
        await enable(true);events('confirm|privileges');
        check(!testing.config.vrchat_chatbox.enabled,'a relaunch must not race an unsafe hot save');
        privileges='ready';testing.events=[];testing.rejectSave=true;
        await enable(true).then(()=>{throw Error('failed save unexpectedly succeeded');},()=>{});events('confirm|privileges|save');
        check(testing.running&&!runtime.getCurrent().vrchat_chatbox.enabled,'save failure preserves existing audio and settings');
        testing.rejectSave=false;testing.events=[];
        await enable(true);events('confirm|privileges|save');
        check(testing.running&&runtime.getCurrent().audio.output.mode==='system','hot enable does not stop or restart audio');
        testing.events=[];await enable(false);events('save');
        testing.running=false;testing.events=[];await enable(true);events('save');
        check(testing.config.vrchat_chatbox.enabled,'idle enable saves without preparation');
        await runtime.update(current=>({...current,audio:{...current.audio,output:{...current.audio.output,mode:'disabled'}}}));
        await wait(()=>capture&&!capture.pending&&runtime.value.audio.output.mode==='disabled');
        testing.events=[];const first=capture.setCaptureRunning(true),second=capture.setCaptureRunning(true);
        check((await first)&&(await second),'concurrent pure-text starts share completion');events('prepare|start');
        await capture.setCaptureRunning(false);testing.events=[];
        const vr=createControlExecutor({update:runtime.update,setCaptureRunning:capture.setCaptureRunning});
        await vr({set_capture_running:true});events('prepare|start');
        check(testing.running,'VR control uses the same pure-text preparation');
        await capture.setCaptureRunning(false);confirmed=false;testing.events=[];
        check(!await capture.setCaptureRunning(true)&&!testing.running,'declined pure-text start never starts capture');events('prepare');
        document.getElementById('result').textContent='PASS';
      }catch(error){document.getElementById('result').textContent='FAIL: '+error.message;}
    ` },
    plugins: [{ name: "settings-fixture", setup(build) {
      build.onResolve({ filter: /^\.\.\/api$/ }, args => args.importer.replaceAll("\\", "/").endsWith("/settings/hooks/useSettingsRuntime.ts")
        ? { path: "settings", namespace: "fixture" } : undefined);
      build.onLoad({ filter: /.*/, namespace: "fixture" }, () => ({ contents:
        `export const settingsApi={settings:async()=>structuredClone(window.testing.config),saveSettings:async next=>{
          const t=window.testing;t.events.push('save');if(t.rejectSave)throw Error('Save failed');
          t.config=structuredClone(next);return structuredClone(next);}};` }));
    } }],
    bundle: true, write: false, outfile: "network.js", format: "esm", platform: "browser", jsx: "automatic",
    define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
  });
  const directory = mkdtempSync(join(testDirectory, ".network-browser-"));
  try {
    const html = join(directory, "test.html");
    writeFileSync(html, '<div id="root"></div><pre id="result">PENDING</pre><script>const report=document.getElementById("result");'
      + 'window.addEventListener("error",e=>report.textContent="FAIL: "+e.message);window.addEventListener("unhandledrejection",e=>report.textContent="FAIL: "+e.reason);</script>'
      + '<script type="module">' + bundle.outputFiles[0].text.replace(/<\/script/gi, '<\\/script') + '</script>');
    const result = spawnSync(browser, ["--headless=new", "--dump-dom", "--virtual-time-budget=10000", "--disable-gpu", "--no-first-run",
      "--disable-background-networking", "--disable-extensions", "--user-data-dir=" + join(directory, "profile"), pathToFileURL(html).href],
    { encoding: "utf8", windowsHide: true, timeout: 30000, maxBuffer: 8 * 1024 * 1024 });
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    const report = result.stdout.match(/<pre id="result">([^<]*)<\/pre>/)?.[1];
    assert.equal(report, "PASS", report || result.stderr);
  } finally {
    assert(relative(testDirectory, directory).startsWith(".network-browser-"));
    rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
});
