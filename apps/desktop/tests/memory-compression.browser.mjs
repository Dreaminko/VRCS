import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve, relative } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const testDirectory = dirname(fileURLToPath(import.meta.url));
const browser = [process.env.PROGRAMFILES, process.env["PROGRAMFILES(X86)"], process.env.LOCALAPPDATA]
  .filter(Boolean).flatMap(root => [join(root, "Google/Chrome/Application/chrome.exe"), join(root, "Microsoft/Edge/Application/msedge.exe")]).find(existsSync);

for (const width of [1100, 380]) test(`memory compression consent, restoration and reboot gate at ${width}px`, {
  timeout: 45000, skip: !browser && "Chrome or Edge is required",
}, async () => {
  const bundle = await build({ stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
    import React,{StrictMode} from 'react';
    import {createRoot} from 'react-dom/client';
    import i18next from 'i18next';
    import {initReactI18next} from 'react-i18next';
    import locale from './src/i18n/locales/zh-CN.json';
    import {usePlayerAudioStart} from './src/capture/usePlayerAudioStart';
    import {MemoryCompressionControls,MemoryCompressionDialog} from './src/capture/MemoryCompressionControls';
    import './src/styles.css';
    await i18next.use(initReactI18next).init({lng:'zh-CN',resources:{'zh-CN':{translation:locale.translation}}});
    window.requestAnimationFrame=callback=>setTimeout(()=>callback(performance.now()),16);
    window.cancelAnimationFrame=clearTimeout;
    const testing=window.testing={enabled:true,elevated:true,restart:false,outcome:'ready',pending:null,events:[],queryFail:false,changeFail:false,running:false};
    const words=locale.translation.settings.audio.memory;
    let gate;
    function Fixture(){
      gate=usePlayerAudioStart({beforeMemoryChange:async()=>{testing.events.push('pause');testing.running=false;return true;}});
      return <><MemoryCompressionControls memory={gate.memory}/>
        {gate.memory.dialog&&<MemoryCompressionDialog memory={gate.memory}/>}</>;
    }
    const root=createRoot(document.getElementById('root'));
    const mount=()=>root.render(<StrictMode><Fixture/></StrictMode>);
    const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
    const wait=async predicate=>{for(let i=0;i<250;i++){await sleep(20);if(predicate())return;}throw Error('Timed out: '+predicate);};
    const check=(value,message)=>{if(!value)throw Error(message);};
    const dialog=()=>document.querySelector('.memory-compression-dialog');
    const click=async label=>{await wait(()=>[...(dialog()?.querySelectorAll('button')??[])].some(button=>button.textContent.trim()===label&&!button.disabled));const button=[...dialog().querySelectorAll('button')].find(button=>button.textContent.trim()===label);check(button&&!button.disabled,'available button: '+label);button.click();};
    const open=async()=>{gate.memory.open();await wait(()=>dialog()&&!gate.memory.busy);};
    const reset=async()=>{gate.memory.close();await wait(()=>!dialog());testing.events=[];};
    const remount=async()=>{root.render(null);await sleep(40);mount();await sleep(80);await wait(()=>gate.memory.status&&!gate.memory.busy);};
    try{
      mount();await wait(()=>gate?.memory.status&&!gate.memory.busy);
      check(gate.memory.status.enabled===true&&testing.events.length===0,'initial detection is read-only');
      check(!await gate.ensurePrivileges(),'enabled compression blocks capture');
      await wait(()=>dialog());
      check(!testing.events.some(value=>value.startsWith('change:')),'capture gate never silently changes a system setting');
      testing.events=[];
      await click(words.disable);await wait(()=>gate.memory.dialog==='disable');
      check(dialog().textContent.includes(words.disableImpact)&&dialog().textContent.includes(words.confirmRestart),'system impact and restart are explained before confirmation');
      await click(locale.translation.common.cancel);await wait(()=>gate.memory.dialog==='manage');
      check(testing.events.length===0&&testing.enabled,'cancellation does not elevate or change Windows');
      await click(words.disable);await wait(()=>gate.memory.dialog==='disable');
      testing.running=true;
      await click(words.confirmDisable);gate.memory.confirmChange();
      await wait(()=>!gate.memory.busy&&gate.memory.status.enabled===false);
      check(testing.events.join('|')==='elevate:disable_memory_compression|pause|change:false','confirmation and elevation precede one system change');
      check(!testing.running&&gate.memory.status.restart_required,'capture stops and a reboot is required');
      check(dialog().textContent.includes(words.restart),'success visibly requests a computer restart');
      check(!await gate.ensurePrivileges(),'restarting VRCS alone cannot bypass the reboot gate');
      await reset();
      await remount();
      check(gate.memory.status.restart_required,'restart reminder survives app relaunch');
      testing.restart=false;
      check(await gate.ensurePrivileges(),'confirmed disabled state after a computer restart allows capture');
      await open();testing.events=[];
      await click(words.enable);await wait(()=>gate.memory.dialog==='enable');
      check(dialog().textContent.includes(words.enableImpact),'restoration has its own impact explanation');
      await click(words.confirmEnable);await wait(()=>!gate.memory.busy&&gate.memory.status.enabled===true);
      check(testing.events.join('|')==='elevate:enable_memory_compression|pause|change:true','restore executes only after explicit confirmation');
      check(gate.memory.status.restart_required,'restoration also requires restart');
      await reset();

      testing.elevated=false;testing.outcome='cancelled';await open();
      check(gate.memory.status.enabled===null,'missing privileges are not reported as disabled compression');
      await click(words.authorize);await wait(()=>!gate.memory.busy&&gate.memory.error);
      check(testing.events.join('|')==='elevate:inspect_memory_compression','cancelled UAC cannot mutate settings');
      check(!gate.resumeRequested,'inspect action cannot request audio capture');
      await reset();
      testing.elevated=true;testing.outcome='ready';testing.restart=false;testing.queryFail=true;
      check(!await gate.ensurePrivileges(),'a detection failure cannot allow capture');
      await wait(()=>dialog()&&gate.memory.error);check(!gate.memory.busy,'query failure releases the UI');
      testing.queryFail=false;await open();testing.events=[];
      await click(words.disable);await wait(()=>gate.memory.dialog==='disable');testing.changeFail=true;
      await click(words.confirmDisable);await wait(()=>!gate.memory.busy&&gate.memory.error);
      check(gate.memory.status.enabled===true&&testing.enabled,'failed changes never show disabled success');
      testing.changeFail=false;await reset();

      // A typed handoff resumes a previously confirmed change exactly once, without capture.
      testing.pending='disable_memory_compression';testing.elevated=true;testing.events=[];
      await remount();await wait(()=>gate.memory.status.enabled===false&&!gate.memory.busy);
      check(testing.events.join('|')==='consume:disable_memory_compression|pause|change:false','StrictMode consumes a confirmed handoff exactly once');
      check(!gate.resumeRequested&&testing.pending===null,'system settings handoff does not start audio');
      await reset();
      testing.pending='enable_memory_compression';testing.events=[];
      await remount();await wait(()=>gate.memory.status.enabled===true&&!gate.memory.busy);
      check(testing.events.join('|')==='consume:enable_memory_compression|pause|change:true','restore handoff preserves the selected operation');
      await reset();
      testing.pending='inspect_memory_compression';testing.events=[];
      await remount();await wait(()=>dialog());
      check(testing.events.join('|')==='consume:inspect_memory_compression'&&!gate.resumeRequested,'inspection handoff only reads status');
      await click(words.disable);await wait(()=>gate.memory.dialog==='disable');
      const bounds=dialog().getBoundingClientRect(),footer=dialog().querySelector('.api-profile-editor-actions').getBoundingClientRect();
      check(bounds.left>=0&&bounds.right<=innerWidth+1&&bounds.top>=0&&footer.bottom<=innerHeight+1,'memory confirmation fits the viewport');
      const copy=dialog().querySelector('.player-audio-start-copy');copy.scrollTop=copy.scrollHeight;
      check(copy.scrollTop+copy.clientHeight>=copy.scrollHeight-1,'the entire impact notice is readable');copy.scrollTop=0;
      document.getElementById('result').textContent='PASS';
    }catch(error){document.getElementById('result').textContent='FAIL: '+error.message;}
  ` }, plugins: [{ name: 'memory-native-fixture', setup(build) {
    build.onResolve({ filter: /^@tauri-apps\/api\/core$/ }, () => ({ path: 'native', namespace: 'fixture' }));
    build.onLoad({ filter: /.*/, namespace: 'fixture' }, () => ({ contents: `
      export const isTauri=()=>true;
      export const invoke=async(name,args)=>{
        const t=window.testing;
        const status=()=>({supported:true,elevated:t.elevated,enabled:t.elevated?t.enabled:null,restart_required:t.elevated&&t.restart});
        if(name==='player_audio_pending_action')return t.pending;
        if(name==='player_audio_consume_action'){const action=t.pending;t.pending=null;t.events.push('consume:'+action);return action;}
        if(name==='player_audio_ensure_privileges'){t.events.push('elevate:'+args.action);return t.outcome;}
        if(name==='player_audio_memory_status'){if(t.queryFail)throw Error('Simulated detection failure');return status();}
        if(name==='player_audio_set_memory_compression'){
          if(!t.elevated)throw Error('Administrator required');
          t.events.push('change:'+args.enabled);
          if(t.changeFail)throw Error('Simulated system setting failure');
          await new Promise(resolve=>setTimeout(resolve,50));
          if(t.enabled!==args.enabled)t.restart=true;t.enabled=args.enabled;return status();
        }
        throw Error('Unexpected native call: '+name);
      };
    ` }));
  } }], bundle: true, write: false, outfile: 'memory-compression.js', format: 'esm', platform: 'browser', jsx: 'automatic' });
  const directory = mkdtempSync(join(testDirectory, '.memory-browser-'));
  try {
    const javascript = bundle.outputFiles.find(file => file.path.endsWith('.js')).text;
    const css = bundle.outputFiles.find(file => file.path.endsWith('.css')).text;
    writeFileSync(join(directory, 'content.html'), '<style>' + css + '</style><div id="root"></div><pre id="result">PENDING</pre><script>const report=document.getElementById("result");window.addEventListener("error",e=>report.textContent="FAIL: "+e.message);window.addEventListener("unhandledrejection",e=>report.textContent="FAIL: "+e.reason);new MutationObserver(()=>parent.postMessage(report.textContent,"*")).observe(report,{childList:true});</script><script type="module">' + javascript.replace(/<\/script/gi, '<\\/script') + '</script>');
    const html = join(directory, 'test.html');
    writeFileSync(html, '<style>body{margin:0}</style><iframe src="content.html" style="display:block;border:0;width:' + width + 'px;height:720px"></iframe><pre id="result">PENDING</pre><script>window.addEventListener("message",e=>document.getElementById("result").textContent=e.data);</script>');
    const screenshot = resolve(testDirectory, '../../../core/target/memory-compression-ui-' + width + '.png');
    const result = spawnSync(browser, ['--headless=new', '--dump-dom', '--virtual-time-budget=15000', '--disable-gpu', '--no-first-run', '--disable-background-networking', '--disable-extensions',
      '--window-size=' + width + ',780', '--screenshot=' + screenshot, '--user-data-dir=' + join(directory, 'profile'), pathToFileURL(html).href], { encoding: 'utf8', windowsHide: true, timeout: 30000, maxBuffer: 8 * 1024 * 1024 });
    assert.ifError(result.error); assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout.match(/<pre id="result">([^<]*)<\/pre>/)?.[1], 'PASS', result.stdout.match(/<pre id="result">([^<]*)<\/pre>/)?.[1] || result.stderr);
  } finally {
    assert(relative(testDirectory, directory).startsWith('.memory-browser-'));
    rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
});
