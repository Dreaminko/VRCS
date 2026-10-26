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

for (const width of [1100, 380]) {
  test(`player audio settings and concurrent previews work at ${width}px`, {
    timeout: 45000, skip: !browser && "Chrome or Edge is required",
  }, async () => {
    const bundle = await build({
      stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
        import React, { useState } from 'react';
        import { createRoot } from 'react-dom/client';
        import { flushSync } from 'react-dom';
        import i18next from 'i18next';
        import { initReactI18next } from 'react-i18next';
        import locale from './src/i18n/locales/zh-CN.json';
        import { AudioSettingsSection } from './src/settings/sections/AudioSettingsSection';
        import { EmptyLiveView, LivePartials } from './src/subtitles/components/LivePartials';
        import { SubtitleBubble } from './src/subtitles/components/SubtitleBubble';
        import { publishSpeakerIdentity } from './src/subtitles/speaker-state';
        import { publishLivePartial, completeLivePartial } from './src/core-client/realtime-state';
        import './src/styles.css';
        await i18next.use(initReactI18next).init({ lng:'zh-CN', resources:{'zh-CN':{translation:locale.translation}} });
        window.requestAnimationFrame = callback => setTimeout(() => callback(performance.now()), 16);
        window.cancelAnimationFrame = clearTimeout;
        const noop = async () => {};
        let current, updateStatus, updateDraft, updateSaveState;
        function Fixture() {
          const [draft,setDraft] = useState({ audio: { output:{mode:'system', device_id:null, trigger_threshold_dbfs:-45},
            microphone:{mode:'disabled',device_id:null,trigger_threshold_dbfs:-45} } });
          current = draft;
          updateDraft = setDraft;
          const [saveState,setSaveState] = useState('idle');
          updateSaveState = setSaveState;
          const [status,setStatus] = useState({phase:'acquiring_key',captured_packets:500,decrypted_packets:0,pcm_frames:0,
            active_players:0,dropped_frames:0,decode_errors:0,detail:null,
            key_scan:{stage:'scanning',scanned_bytes:4096*1048576,total_bytes:8455*1048576,
              candidates_tested:42,elapsed_ms:2700,scan_elapsed_ms:2000,workers:8}});
          updateStatus = setStatus;
          return <><AudioSettingsSection draft={draft} devices={[]} devicesReady={true} deviceErrors={[]}
            outputDevices={[]} microphoneDevices={[]} microphoneRunning={false} transcriptionRunning={false}
            microphoneTestRunning={false} saveState={saveState} onRefresh={noop} applySettings={setDraft}
            onStartMicrophoneTest={noop} onStopMicrophoneTest={noop}
            playerAudioStatus={status} />
            <div id="waiting"><EmptyLiveView running={true} playerAudioPhase={status.phase} /></div>
            <div id="previews"><LivePartials subtitles={[]} playerAudioPhase={status.phase} /></div>
            <div id="history"><SubtitleBubble subtitle={{id:1,text:'已保存的玩家发言',language:'zh',created_at:'2026-10-25T00:00:00Z',
              started_at:null,ended_at:null,source:'speaker',speaker:{id:'actor-1',index:1},translations:[]}}
              focused={false} selected={false} selectionActive={false} selection={null} onClearSelection={()=>{}}
              onSelect={noop} onCopyFeedback={()=>{}} /></div></>;
        }
        createRoot(document.getElementById('root')).render(<Fixture />);
        const wait = async predicate => {
          for (let i=0;i<150;i++) { if(predicate())return; await new Promise(r=>setTimeout(r,20)); }
          throw Error('Timed out: '+predicate);
        };
        const check = (ok, message) => { if(!ok)throw Error(message); };
        const inputRange = (input, value) => flushSync(() => {
          Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, String(value));
          input.dispatchEvent(new Event('input', { bubbles:true }));
        });
        const commitRange = (input, event) => flushSync(() => input.dispatchEvent(event));
        try {
          await wait(() => document.querySelector('.device-select .dropdown-trigger'));
          const name=locale.translation.settings.audio.players.title;
          document.querySelector('.device-select .dropdown-trigger').click();
          await wait(()=>document.querySelector('[role=option]'));
          [...document.querySelectorAll('[role=option]')].find(e=>e.querySelector('.dropdown-option-copy > span')?.textContent===name).click();
          await wait(()=>document.querySelector('.player-audio-settings'));
          check(current.audio.output.mode==='vrchat_players','experimental source saved');
          const card=document.querySelector('.player-audio-settings');
          const controls=['limit','visibleLimit'].map(key=>card.querySelector('input[type="range"][aria-label="'+locale.translation.settings.audio.players[key]+'"]'));
          check(controls.every(Boolean),'both player counts use labeled sliders');
          const [processing,visible]=controls;
          const unlimited=locale.translation.settings.audio.players.unlimited;
          check(processing.value==='0'&&processing.getAttribute('aria-valuetext')===unlimited,'default processing limit remains unlimited');
          check(visible.value==='4'&&visible.getAttribute('aria-valuetext')==='4','default visible limit remains four players');
          for (const [input,field,max,initial,selection] of [
            [processing,'max_players',8,0,2],
            [visible,'max_visible_players',64,4,12],
          ]) {
            check(input.min==='0'&&input.max===String(max)&&input.step==='1','player sliders preserve the full integer range');
            const previous=current.audio.output.player_audio;
            inputRange(input,selection);
            check(input.closest('label').querySelector('output').textContent===String(selection),'the count updates while dragging');
            check((current.audio.output.player_audio?.[field]??initial)===initial,'dragging does not save intermediate values');
            commitRange(input,new PointerEvent('pointerup',{bubbles:true}));
            check(current.audio.output.player_audio[field]===selection,'releasing the slider saves the selected count');
            const other=field==='max_players'?'max_visible_players':'max_players';
            check(current.audio.output.player_audio[other]===(previous?.[other]??(other==='max_players'?0:4)),'each slider preserves the other limit');
            inputRange(input,max);
            commitRange(input,new KeyboardEvent('keyup',{key:'End',bubbles:true}));
            check(current.audio.output.player_audio[field]===max,'keyboard completion saves the maximum count');
            inputRange(input,0);
            commitRange(input,new FocusEvent('focusout',{bubbles:true}));
            check(current.audio.output.player_audio[field]===0&&input.getAttribute('aria-valuetext')===unlimited,'zero retains the unlimited option and saves on blur');
          }
          updateDraft(previous=>({...previous,audio:{...previous.audio,output:{...previous.audio.output,
            player_audio:{...previous.audio.output.player_audio,max_players:3,max_visible_players:10}}}}));
          await wait(()=>processing.value==='3'&&visible.value==='10');
          updateSaveState('saving');
          await wait(()=>controls.every(input=>input.disabled));
          updateSaveState('idle');
          await wait(()=>controls.every(input=>!input.disabled));
          const statusText=()=>card.querySelector('[role=status]').textContent;
          check(statusText().includes('正在获取语音密钥')&&statusText().includes('已用时 2.7 秒'),'acquisition shows a plain status and elapsed time');
          check(!/MiB|PCM|认证包|候选|解码错误|丢弃/.test(statusText()),'diagnostic counters are hidden');
          for (const [phase,label] of [['waiting_game','等待 VRChat 启动'],['waiting_world','等待 VRChat 世界加载完成'],['retrying_key','密钥获取未成功，稍后自动重试']]) {
            updateStatus(previous=>({...previous,phase}));
            await wait(()=>card.querySelector('[role=status]').textContent.includes(label));
            await wait(()=>document.querySelector('#waiting').textContent.includes(label));
            check(!statusText().includes('已用时'),'waiting and retrying do not show stale scan progress');
          }
          updateStatus(previous=>({...previous,phase:'streaming',decrypted_packets:480,pcm_frames:256,active_players:2,
            key_scan:{...previous.key_scan,stage:'verified',elapsed_ms:4200}}));
          await wait(()=>statusText().includes('语音密钥已获取'));
          check(statusText().includes('已接入 2 位玩家的音轨')&&statusText().includes('耗时 4.2 秒'),'connected tracks and successful acquisition time are visible');
          check(!statusText().includes('480')&&!/MiB|PCM|认证包|候选/.test(statusText()),'completed acquisition does not retain diagnostic counters');
          updateStatus(previous=>({...previous,active_players:0,decrypted_packets:385,pcm_frames:0,
            key_scan:{...previous.key_scan,elapsed_ms:9200}}));
          await wait(()=>statusText().includes('等待玩家语音'));
          check(statusText().includes('耗时 9.2 秒')&&!statusText().includes('0 位'),'no active tracks is a waiting state, not a row of zero counters');
          updateStatus(previous=>({...previous,key_scan:{...previous.key_scan,stage:'cached',total_bytes:0,elapsed_ms:1}}));
          await wait(()=>statusText().includes('已复用本次登录的密钥'));
          check(!statusText().includes('耗时'),'cached keys do not show a meaningless acquisition time');
          const selectTracks=selected_tracks=>updateDraft(previous=>({...previous,audio:{...previous.audio,
            output:{...previous.audio.output,player_audio:{...previous.audio.output.player_audio,selected_tracks}}}}));
          selectTracks([]);
          await wait(()=>statusText().includes(locale.translation.live.players.noneSelected));
          selectTracks(null);
          await wait(()=>statusText().includes('等待玩家语音'));
          updateStatus(previous=>({...previous,phase:'stopped'}));
          await wait(()=>statusText()==='已停止');
          updateStatus(previous=>({...previous,phase:'failed',detail:'Network capture unavailable'}));
          await wait(()=>statusText().includes('Network capture unavailable'));
          check(!statusText().includes('密钥')&&card.querySelector('.settings-validation-error'),'failures retain the actionable error without stale success details');
          updateStatus(previous=>({...previous,phase:'streaming',active_players:2,detail:null}));
          await wait(()=>statusText().includes('已接入 2 位玩家的音轨'));
          for (const [index,text] of ['第一位玩家','第二位玩家'].entries()) publishLivePartial({
            type:'partial',source:'speaker',utterance_id:'player-'+index,text,language:'zh',translation:'Player '+index,
            target_language:'en',speaker:{id:'actor-'+index,index} });
          await wait(()=>document.querySelectorAll('#previews .streaming-message').length===2);
          const vrcName='玩家 <VRC> あ '+ '很长的名字'.repeat(12);
          publishSpeakerIdentity({id:'actor-1',index:1,display_name:vrcName});
          await wait(()=>document.querySelector('#history .speaker-name')?.textContent===vrcName);
          await wait(()=>[...document.querySelectorAll('#previews .speaker-name')].some(e=>e.textContent===vrcName));
          check(document.querySelectorAll('#previews .streaming-message').length===2,'name update preserves all previews');
          check(!document.querySelector('#history vrc'),'name renders as literal text');
          check(document.querySelector('#history .bubble-original').textContent==='已保存的玩家发言','name update preserves saved text');
          check(!document.querySelector('#history .message-meta').textContent.includes('说话人 2'),'known name replaces numbered label');
          completeLivePartial('speaker','player-0');
          await wait(()=>document.querySelectorAll('#previews .streaming-message').length===1);
          check(document.querySelector('#previews').textContent.includes('第二位玩家'),'other player survives completion');
          check(document.documentElement.scrollWidth<=innerWidth+1,'no horizontal overflow');
          document.getElementById('result').textContent='PASS';
        } catch(error) { document.getElementById('result').textContent='FAIL: '+error.message; }
      ` },
      plugins: [{ name:"locale-fixture", setup(build) {
        build.onResolve({ filter:/i18n\/catalog$/ }, () => ({ path:"locale", namespace:"fixture" }));
        build.onLoad({ filter:/.*/, namespace:"fixture" }, () => ({ resolveDir:join(testDirectory,".."),
          contents:'import locale from "./src/i18n/locales/zh-CN.json"; export const localeCatalog=[locale]; export const supportedUiLocales=["zh-CN"]; export const localeResources={"zh-CN":{translation:locale.translation}};' }));
      } }],
      bundle:true, write:false, outfile:"player-audio.js", format:"esm", platform:"browser", jsx:"automatic",
      define:{"import.meta.env.VITE_VRCS_SESSION_TOKEN":'""'},
    });
    const directory=mkdtempSync(join(testDirectory,".player-audio-browser-"));
    try {
      const javascript=bundle.outputFiles.find(f=>f.path.endsWith(".js")).text;
      const css=bundle.outputFiles.find(f=>f.path.endsWith(".css")).text;
      writeFileSync(join(directory,"content.html"), '<style>'+css+'</style><div class="settings-surface" id="root"></div><pre id="result">PENDING</pre>'
        +'<script>const report=document.getElementById("result"); window.addEventListener("error",e=>report.textContent="FAIL: "+e.message); window.addEventListener("unhandledrejection",e=>report.textContent="FAIL: "+e.reason); new MutationObserver(()=>parent.postMessage(report.textContent,"*")).observe(report,{childList:true});</script>'
        +'<script type="module">'+javascript.replace(/<\/script/gi,'<\\/script')+'</script>');
      const html=join(directory,"test.html");
      writeFileSync(html, '<iframe src="content.html" style="display:block;border:0;width:'+width+'px;height:2300px"></iframe><pre id="result">PENDING</pre>'
        +'<script>window.addEventListener("message",e=>document.getElementById("result").textContent=e.data);</script>');
      const screenshot=resolve(testDirectory, '../../../core/target/player-audio-ui-'+width+'.png');
      const result=spawnSync(browser,["--headless=new","--dump-dom","--virtual-time-budget=10000","--disable-gpu","--no-first-run",
        "--disable-background-networking","--disable-extensions",`--window-size=${width},2300`,"--screenshot="+screenshot,
        "--user-data-dir="+join(directory,"profile"),pathToFileURL(html).href],{encoding:"utf8",windowsHide:true,timeout:30000,maxBuffer:8*1024*1024});
      assert.ifError(result.error);
      assert.equal(result.status,0,result.stderr);
      const report=result.stdout.match(/<pre id="result">([^<]*)<\/pre>/)?.[1];
      assert.equal(report,"PASS",report || result.stderr);
    } finally {
      assert(relative(testDirectory,directory).startsWith(".player-audio-browser-"));
      rmSync(directory,{recursive:true,force:true,maxRetries:10,retryDelay:200});
    }
  });
}
