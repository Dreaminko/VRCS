import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve, relative } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const testDirectory=dirname(fileURLToPath(import.meta.url));
const browser=[process.env.PROGRAMFILES,process.env['PROGRAMFILES(X86)'],process.env.LOCALAPPDATA].filter(Boolean)
  .flatMap(root=>[join(root,'Google/Chrome/Application/chrome.exe'),join(root,'Microsoft/Edge/Application/msedge.exe')]).find(existsSync);

for(const width of [1100,380]) test(`${width===1100?'player audio consent, UAC cancellation and resume':'player audio dialog layout'} at ${width}px`,{timeout:45000,skip:!browser&&'Chrome or Edge is required'},async()=>{
  const fullScenario=width===1100;
  const bundle=await build({stdin:{resolveDir:join(testDirectory,'..'),loader:'tsx',contents:`
    import React,{StrictMode,useState} from 'react';
    import {createRoot} from 'react-dom/client';
    import i18next from 'i18next';
    import {initReactI18next} from 'react-i18next';
    import locale from './src/i18n/locales/zh-CN.json';
    import {usePlayerAudioStart} from './src/capture/usePlayerAudioStart';
    import {PlayerAudioStartDialog} from './src/capture/PlayerAudioStartDialog';
    import {createCaptureController} from './src/capture/capture-control';
    import {useSettingsRuntime} from './src/settings/hooks/useSettingsRuntime';
    import './src/styles.css';
    await i18next.use(initReactI18next).init({lng:'zh-CN',resources:{'zh-CN':{translation:locale.translation}}});
    window.requestAnimationFrame=callback=>setTimeout(()=>callback(performance.now()),16);
    window.cancelAnimationFrame=clearTimeout;
    const testing=window.testing={running:false,outcome:'ready',resume:false,events:[],errors:[],
      config:{features:{},audio:{output:{mode:'vrchat_players',device_id:null},microphone:{mode:'disabled',device_id:null}}}};
    const health={getCurrent:()=>({capture_requested:testing.running}),refreshQuietly:async()=>{}};
    const setRunning=async target=>{testing.events.push((target?'start:':'stop:')+testing.config.audio.output.mode);testing.running=target;return true;};
    const noop=()=>{};
    let gate,settings,changePlayers;
    const control=createCaptureController({available:()=>true,running:()=>testing.running,newConversation:()=>true,
      beforeStart:()=>gate.prepare(),createConversation:async()=>{testing.events.push('conversation');return true;},setRunning,onPending:noop});
    function Fixture(){
      const [players,setPlayers]=useState([]);
      changePlayers=setPlayers;
      gate=usePlayerAudioStart();
      settings=useSettingsRuntime({active:false,coreConfigured:true,health:{...health,value:{player_audio:{players}}},stopMicrophoneTest:async()=>{},setCaptureRunning:setRunning,
        playerAudioStart:gate,clearErrorFrom:noop,reportError:error=>testing.errors.push(String(error))});
      return <><button id="start" onClick={()=>void control.setRunning(true).catch(error=>testing.errors.push(String(error)))}>开始采集</button>
        {gate.notice&&<PlayerAudioStartDialog openedAt={gate.notice.openedAt} onDecision={gate.decide}/>}</>;
    }
    const root=createRoot(document.getElementById('root'));
    root.render(<StrictMode><Fixture/></StrictMode>);
    const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
    const wait=async predicate=>{for(let i=0;i<250;i++){if(predicate())return;await sleep(20);}throw Error('Timed out: '+predicate);};
    const check=(value,message)=>{if(!value)throw Error(message);};
    const dialog=()=>document.querySelector('.player-audio-start-dialog');
    const accept=()=>dialog()?.querySelector('.primary-button');
    const cancel=()=>dialog()?.querySelector('.api-profile-editor-actions .secondary-button');
    const mode=value=>settings.update(current=>({...current,audio:{...current.audio,output:{...current.audio.output,mode:value}}}));
    try{
      await wait(()=>settings?.value);
      if(${fullScenario}){
      const user='usr_00000000-0000-4000-8000-000000000007';
      await settings.update(current=>({...current,audio:{...current.audio,output:{...current.audio.output,player_audio:{
        pagefile_drive:'C:',opus_library_path:'',max_players:0,selected_tracks:[user],selected_user_names:{[user]:'Old name'},
      }}}}));
      const known={speaker:{id:'vrc:new-world:42',index:42,user_id:user,display_name:'Renamed player'},active:false,selected:true};
      changePlayers([known]);
      await wait(()=>testing.config.audio.output.player_audio.selected_user_names[user]==='Renamed player');
      check(settings.value.audio.output.player_audio.selected_tracks[0]===user,'rename refresh preserves the selected account');
      const writeCount=testing.events.length;
      changePlayers([structuredClone(known)]);await sleep(100);
      check(testing.events.length===writeCount,'unchanged names do not write settings again');
      changePlayers([{...known,speaker:{...known.speaker,user_id:null,display_name:'Unknown'}}]);await sleep(100);
      check(testing.config.audio.output.player_audio.selected_user_names[user]==='Renamed player','missing identity cannot overwrite a saved name');
      testing.events=[];
      document.getElementById('start').click();document.getElementById('start').click();
      await wait(()=>dialog());
      check(accept().disabled&&cancel().disabled,'all decisions are disabled during reading');
      gate.decide(true);
      document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));
      document.querySelector('.api-profile-dialog-backdrop').dispatchEvent(new MouseEvent('mousedown',{bubbles:true}));
      await sleep(200);
      check(dialog()&&testing.events.length===0,'no early close, UAC, conversation or capture');
      await wait(()=>accept()&&!accept().disabled);
      check(performance.now()-gate.notice.openedAt>=3000,'minimum reading time is three seconds');
      accept().click();
      await wait(()=>testing.running);
      check(testing.events.join('|')==='elevate|conversation|start:vrchat_players','UAC precedes conversation and capture');
      await control.setRunning(false);
      await mode('system');testing.running=true;testing.events=[];
      const cancelled=mode('vrchat_players');
      await wait(()=>dialog());await wait(()=>!cancel().disabled);cancel().click();await cancelled;
      check(testing.config.audio.output.mode==='system'&&testing.running&&testing.events.length===0,'declining leaves the original running input unchanged');
      testing.outcome='cancelled';
      const uacCancelled=mode('vrchat_players');
      await wait(()=>dialog());await wait(()=>!accept().disabled);accept().click();await uacCancelled;
      check(testing.events.join('|')==='stop:system|save:vrchat_players|elevate|save:system|start:system','cancelled UAC restores the original mode');
      check(testing.config.audio.output.mode==='system'&&testing.running,'original capture is still available after UAC cancellation');
      testing.outcome='ready';testing.events=[];
      const switched=mode('vrchat_players');
      await wait(()=>dialog());await wait(()=>!accept().disabled);accept().click();await switched;
      check(testing.events.join('|')==='stop:system|save:vrchat_players|elevate|start:vrchat_players','active mode switch cannot bypass the consent and privilege gate');
      // React mount probes must not consume the one-time handoff before Core is ready.
      root.render(null);await sleep(30);testing.resume=true;testing.running=false;testing.events=[];
      root.render(<StrictMode><Fixture/></StrictMode>);
      await wait(()=>gate.resumeRequested&&settings?.value);
      check(settings.value.audio.output.player_audio.selected_user_names[user]==='Renamed player','renamed labels survive an application remount');
      await control.setRunning(true);
      check(!dialog()&&testing.running,'elevated handoff continues without asking the same consent twice');
      check(testing.events.join('|')==='consume|conversation|start:vrchat_players','resume is consumed exactly once');
      await control.setRunning(false);
      document.getElementById('start').click();await wait(()=>dialog());await sleep(100);
      check(accept().disabled,'each later start has a fresh reading interval');
      }else{
        void gate.confirm();await wait(()=>dialog());
      }
      const bounds=dialog().getBoundingClientRect(),footer=dialog().querySelector('.api-profile-editor-actions').getBoundingClientRect();
      check(bounds.left>=0&&bounds.right<=innerWidth+1&&bounds.top>=0&&footer.bottom<=innerHeight+1,'dialog and action buttons stay in the viewport');
      const copy=dialog().querySelector('.player-audio-start-copy');
      check(copy.clientHeight>80,'notice has a usable scrolling region');
      copy.scrollTop=copy.scrollHeight;
      check(copy.scrollTop+copy.clientHeight>=copy.scrollHeight-1,'all disclaimer content is reachable');
      copy.scrollTop=0;
      document.getElementById('result').textContent='PASS';
    }catch(error){document.getElementById('result').textContent='FAIL: '+error.message;}
  `},plugins:[{name:'native-and-settings-fixtures',setup(build){
    build.onResolve({filter:/^@tauri-apps\/api\/core$/},()=>({path:'native',namespace:'fixture'}));
    build.onResolve({filter:/^\.\.\/api$/},args=>args.importer.replaceAll('\\','/').endsWith('/settings/hooks/useSettingsRuntime.ts')?{path:'settings',namespace:'fixture'}:undefined);
    build.onLoad({filter:/.*/,namespace:'fixture'},args=>({contents:args.path==='native'?
      `export const isTauri=()=>true;export const invoke=async name=>{const t=window.testing;if(name==='player_audio_pending_action')return t.resume?'start_capture':null;if(name==='player_audio_consume_action'){t.events.push('consume');const result=t.resume?'start_capture':null;t.resume=false;return result;}if(name==='player_audio_memory_status')return {supported:true,elevated:true,enabled:false,restart_required:false};if(name==='player_audio_ensure_privileges'){t.events.push('elevate');return t.outcome;}throw Error('Unexpected native call: '+name);};`:
      `export const settingsApi={settings:async()=>structuredClone(window.testing.config),saveSettings:async next=>{window.testing.events.push('save:'+next.audio.output.mode);window.testing.config=structuredClone(next);return structuredClone(next);}};`}));
  }}],bundle:true,write:false,outfile:'player-audio-start.js',format:'esm',platform:'browser',jsx:'automatic',define:{'import.meta.env.VITE_VRCS_SESSION_TOKEN':'""'}});
  const directory=mkdtempSync(join(testDirectory,'.player-start-browser-'));
  try{
    const javascript=bundle.outputFiles.find(file=>file.path.endsWith('.js')).text;
    const css=bundle.outputFiles.find(file=>file.path.endsWith('.css')).text;
    writeFileSync(join(directory,'content.html'),'<style>'+css+'</style><div id="root"></div><pre id="result">PENDING</pre><script>const report=document.getElementById("result");window.addEventListener("error",e=>report.textContent="FAIL: "+e.message);window.addEventListener("unhandledrejection",e=>report.textContent="FAIL: "+e.reason);new MutationObserver(()=>parent.postMessage(report.textContent,"*")).observe(report,{childList:true});</script><script type="module">'+javascript.replace(/<\/script/gi,'<\\/script')+'</script>');
    const html=join(directory,'test.html');
    writeFileSync(html,'<style>body{margin:0}</style><iframe src="content.html" style="display:block;border:0;width:'+width+'px;height:720px"></iframe><pre id="result">PENDING</pre><script>window.addEventListener("message",e=>document.getElementById("result").textContent=e.data);</script>');
    const screenshot=resolve(testDirectory,'../../../core/target/player-audio-start-ui-'+width+'.png');
    const result=spawnSync(browser,['--headless=new','--dump-dom','--virtual-time-budget=20000','--disable-gpu','--no-first-run','--disable-background-networking','--disable-extensions',
      '--window-size='+width+',780','--screenshot='+screenshot,'--user-data-dir='+join(directory,'profile'),pathToFileURL(html).href],{encoding:'utf8',windowsHide:true,timeout:30000,maxBuffer:8*1024*1024});
    assert.ifError(result.error);assert.equal(result.status,0,result.stderr);
    assert.equal(result.stdout.match(/<pre id="result">([^<]*)<\/pre>/)?.[1],'PASS',result.stdout.match(/<pre id="result">([^<]*)<\/pre>/)?.[1]||result.stderr);
  }finally{
    const cleanupDirectory=resolve(directory);
    assert.equal(dirname(cleanupDirectory),resolve(testDirectory));
    assert(relative(resolve(testDirectory),cleanupDirectory).startsWith('.player-start-browser-'));
    rmSync(cleanupDirectory,{recursive:true,force:true,maxRetries:10,retryDelay:200});
  }
});
