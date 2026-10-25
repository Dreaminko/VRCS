import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve, relative } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const testDirectory = dirname(fileURLToPath(import.meta.url));
const browser = [process.env.PROGRAMFILES, process.env["PROGRAMFILES(X86)"], process.env.LOCALAPPDATA]
  .filter(Boolean).flatMap(root => [join(root,"Google/Chrome/Application/chrome.exe"),join(root,"Microsoft/Edge/Application/msedge.exe")]).find(existsSync);


for (const mode of ["compact", "compact-qwen", "compact-filter", "headset", "wrist"]) {
  test(`independent player captions on ${mode}`, { timeout: 45000, skip: !browser && "Chrome or Edge is required" }, async () => {
    const source = String.raw`
      import React from 'react';
      import {createRoot} from 'react-dom/client';
      import {flushSync} from 'react-dom';
      import i18next from 'i18next';
      import {initReactI18next} from 'react-i18next';
      import locale from './src/i18n/locales/zh-CN.json';
      import {CompactView} from './src/shell/CompactView';
      import {COMPACT_LAYOUT,COMPACT_FONT_PRESETS,playerCaptionMinimumHeight} from './src/shell/compact-layout';
      import {VrSurface} from './src/vr-surface/VrSurface';
      import {publishLivePartial,completeLivePartial,clearLivePartials,publishTranslationPartial,clearTranslationPartials} from './src/core-client/realtime-state';
      import {usePlayerHistoryPreferences} from './src/subtitles/usePlayerHistoryPreferences';
      import {PLAYER_HISTORY_KEY} from './src/subtitles/player-history';
      import {publishSpeakerIdentity} from './src/subtitles/speaker-state';
      import './src/styles.css';
      const mode=MODE_PLACEHOLDER;
      const isCompact=mode.startsWith('compact');
      await i18next.use(initReactI18next).init({lng:'zh-CN',resources:{'zh-CN':{translation:locale.translation}}});
      const root=createRoot(document.getElementById('root'));
      const check=(value,message)=>{if(!value)throw new Error(message);};
      const wait=async(predicate)=>{const end=Date.now()+5000;while(!predicate()){if(Date.now()>end)throw new Error('Timed out: '+predicate);await new Promise(resolve=>setTimeout(resolve,20));}};
      const noop=()=>{};
      const players=Array.from({length:8},(_,index)=>({id:'player-'+index,index,display_name:['Alice','Bob','千夏','小明'][index]||'Player '+index}));
      const preview=(index,text='Hello from '+players[index].display_name)=>({type:'partial',source:'speaker',speaker:players[index],utterance_id:'player-'+index+(mode==='compact-qwen'?'-qwen-preview-':'-')+'utterance',text,language:'en',translation:'来自 '+players[index].display_name+' 的译文',target_language:'zh'});
      const heightRequests=[];
      const compactProps={onCaptionHeight:height=>heightRequests.push(height),transparentBackground:false,onToggleBackground:noop,backgroundOpacity:100,onBackgroundOpacityChange:noop,fontSize:'medium',onFontSizeChange:noop,subtitles:[],subtitleHistory:[],selectionTarget:null,running:true,vrchatMuted:false,captureDisabled:false,onSelect:async()=>{},onCapture:noop,onRestore:noop,onClose:noop,onResize:noop};
      const preset=COMPACT_FONT_PRESETS.medium;
      const vars={...Object.fromEntries(Object.entries(COMPACT_LAYOUT).map(([key,value])=>['--compact-'+key.replace(/[A-Z]/g,letter=>'-'+letter.toLowerCase()),value+'px'])),
        '--compact-original-size':preset.original+'px','--compact-original-line':preset.originalLine+'px','--compact-translation-size':preset.translation+'px','--compact-translation-line':preset.translationLine+'px',width:720,height:playerCaptionMinimumHeight(4,'medium')};
      const compact=(limit)=>flushSync(()=>root.render(<div className="compact-root" style={vars}><CompactView {...compactProps} playerLimit={limit}/></div>));
      let revision=0;
      const surface=(messages,notice=null,dark=false,selection={})=>{
        const labels={me:'我',other:'其他人'};
        const view=mode==='headset'?{kind:'headset',dark,messages,notice,labels,lines_per_language:2,font_size:48,opacity:0.95,...selection}:
          {kind:'dialogue',parallel:true,controls:{owner_epoch:1,revision:1,dark,labels:{dialogueTitle:'手腕对话'},items:[]},messages,notice,labels,mode:'wrist',follow_view:false,busy:false,error:null,font_size:28,opacity:0.94,...selection};
        flushSync(()=>root.render(<VrSurface state={{key:{generation:1,revision:++revision},view,hovered:null,pressed:null,pointer:null}} onLayout={noop}/>));
      };
      try{
        if(mode==='compact-filter'){
          localStorage.removeItem(PLAYER_HISTORY_KEY);
          let policy;
          function Workspace({scene,targets}){
            policy=usePlayerHistoryPreferences({speaker_targets:targets.map(target_language=>({target_language}))});
            return scene==='chat'?<div>Chat</div>:<div className="compact-root" style={vars}><CompactView {...compactProps} playerLimit={2} hiddenTracks={policy.hiddenTracks} hiddenSourceLanguages={policy.hiddenSourceLanguages}/></div>;
          }
          const render=(scene='compact',targets=['zh-CN','ja'])=>flushSync(()=>root.render(<Workspace scene={scene} targets={targets}/>));
          render('chat');
          flushSync(()=>policy.update({hiddenTracks:['player-0'],hiddenSourceLanguages:null}));
          for(let i=0;i<5;i++)publishLivePartial({...preview(i),language:['en','zh','ja','en','fr'][i]});
          publishLivePartial({...preview(7),utterance_id:'my-microphone',source:'microphone',speaker:null,language:'zh',text:'自己的中文',translation:'My words'});
          render();
          const visible=()=>document.querySelector('.compact-subtitle-stack').textContent;
          await wait(()=>visible().includes('小明')&&visible().includes('Player 4')&&visible().includes('自己的中文'));
          check(!visible().includes('Alice')&&!visible().includes('Bob')&&!visible().includes('千夏'),'Hidden live tracks and all default languages are excluded before the cap');
          render('chat');render('compact',['fr','ja']);
          await wait(()=>visible().includes('Bob')&&visible().includes('小明'));
          check(!visible().includes('Player 4'),'Changed default targets are live after switching modes');
          flushSync(()=>policy.update({hiddenTracks:['player-0'],hiddenSourceLanguages:['en','fr']}));
          await wait(()=>visible().includes('Bob')&&visible().includes('千夏'));
          render('chat');render();
          check(policy.preferences.hiddenSourceLanguages.join(',')==='en,fr','Manual multi-select persists across modes');
          flushSync(()=>root.render(null));render();
          check(policy.preferences.hiddenTracks[0]==='player-0'&&policy.preferences.hiddenSourceLanguages.join(',')==='en,fr','Shared preferences reload from local storage');
          clearLivePartials();
          const chinese={id:91,utterance_id:'player-1-qwen-source-final',source:'speaker',speaker:players[1],text:'今天我们准备检查中文过滤功能。',language:null,translations:[],created_at:'2026-10-25T00:00:00Z'};
          const english={...chinese,id:92,utterance_id:'english',speaker:players[3],text:'This English original remains visible.',language:'en',translations:[{target_language:'zh',text:'中文翻译仍然显示'}]};
          compactProps.subtitles=[chinese,english];compactProps.subtitleHistory=[chinese,english];
          flushSync(()=>policy.update({hiddenTracks:[],hiddenSourceLanguages:null}));render();
          publishLivePartial({...preview(1,''),utterance_id:'player-1-qwen-preview-late',source_utterance_id:chinese.utterance_id,language:null,translation:'A late English translation'});
          publishTranslationPartial(91,{target_language:'en',text:'Another late translation',preferred:true});
          await wait(()=>visible().includes('This English original')&&visible().includes('中文翻译仍然显示'));
          await new Promise(resolve=>setTimeout(resolve,120));
          check(!visible().includes('Bob')&&!visible().includes('今天我们准备')&&!visible().includes('late'),'Hidden completed source cannot return through a late translation');
          clearLivePartials();clearTranslationPartials();
        }else if(isCompact){
          for(let i=0;i<8;i++)publishLivePartial(preview(i));
          compact(4);
          await wait(()=>document.querySelectorAll('.compact-parallel .compact-subtitle-row').length===4);
          const cards=()=>[...document.querySelectorAll('.compact-subtitle-stack .compact-subtitle-row')];
          check(cards().map(card=>card.querySelector('.compact-speaker').textContent).join(',')==='Alice,Bob,千夏,小明','All visible players have names in stable order');
          await wait(()=>heightRequests.includes(playerCaptionMinimumHeight(4,'medium')));
          const viewport=document.querySelector('.compact-content').getBoundingClientRect();
          check(cards().every(card=>{const rect=card.getBoundingClientRect();return rect.top>=viewport.top-1&&rect.bottom<=viewport.bottom+1&&rect.left>=viewport.left-1&&rect.right<=viewport.right+1;}),'All four lanes fit the automatically expanded caption window');
          check(cards().every(card=>card.querySelector('.compact-speaker').getBoundingClientRect().height>=14),'Speaker names must not shrink or clip');
          check(cards().every((card,index)=>index===0||card.getBoundingClientRect().top>=cards()[index-1].getBoundingClientRect().bottom),'Desktop lanes are stacked vertically');
          check(cards().every(card=>card.querySelector('.compact-original').clientHeight>0&&card.querySelector('.compact-translation').clientHeight>0),'Every player retains both languages');
          publishLivePartial(preview(1,'Updated Bob only'));
          await wait(()=>cards()[1].textContent.includes('Updated Bob only'));
          check(cards()[0].textContent.includes('Hello from Alice'),'Another player retains their text');
          publishSpeakerIdentity({...players[1],display_name:'Bob 改名'});
          await wait(()=>cards()[1].querySelector('.compact-speaker').textContent==='Bob 改名');
          compact(2);await wait(()=>cards().length===2);
          completeLivePartial('speaker',preview(0).utterance_id);await wait(()=>cards()[0].textContent.includes('Bob 改名'));
          compact(0);await wait(()=>cards().length===7);
          check(cards().length===7,'Unlimited display keeps every live player');
          clearLivePartials();await wait(()=>!document.querySelector('.compact-parallel'));
          if(mode==='compact-qwen'){
            const subtitle={id:42,utterance_id:'player-1-qwen-source-final',text:'Completed source',source:'speaker',speaker:players[1],language:'en',created_at:'2026-10-25T00:00:00Z',translations:[]};
            compactProps.subtitles=[subtitle];compactProps.subtitleHistory=[subtitle];
            publishTranslationPartial(42,{target_language:'zh',text:'正在补全译文',preferred:true});
            compact(4);await wait(()=>cards().some(card=>card.textContent.includes('正在补全译文')));
            for(let i=0;i<5;i++){compactProps.subtitles=[{...subtitle}];compact(4);await new Promise(resolve=>setTimeout(resolve,60));}
            check(cards().some(card=>card.textContent.includes('Completed source')),'Equivalent candidate IDs remain stable across rerenders');
            clearTranslationPartials();compactProps.subtitles=[];compactProps.subtitleHistory=[];
          }
          const finalC={id:999,utterance_id:'player-2'+(mode==='compact-qwen'?'-qwen-source-':'-')+'final',source:'speaker',speaker:players[2],text:'Finished C original',language:'en',created_at:'2026-10-25T00:00:00Z',translations:[]};
          const oldC={...finalC,id:998,utterance_id:'older-C',text:'Older C original'};
          const oldA=Array.from({length:40},(_,index)=>({...finalC,id:1000+index,utterance_id:'older-A-'+index,speaker:players[0],text:'Older A original '+index}));
          compactProps.subtitles=[oldC,finalC,...oldA];compactProps.subtitleHistory=compactProps.subtitles;
          publishLivePartial(preview(0,'A keeps speaking'));publishLivePartial(preview(1,'B keeps speaking'));
          compact(4);
          await wait(()=>cards().length===3&&cards().some(card=>card.textContent.includes('Finished C original')));
          check(cards().map(card=>card.querySelector('.compact-speaker').textContent).join(',')==='Alice,Bob 改名,千夏','Completed and live captions retain stable player lanes');
          check(!cards().some(card=>card.textContent.includes('Older')),'Only the latest caption per track is retained, and previews replace their own track');
          await wait(()=>heightRequests.at(-1)===playerCaptionMinimumHeight(3,'medium'));
          publishTranslationPartial(999,{target_language:'zh',text:'C final translation',preferred:true});
          await wait(()=>cards().some(card=>card.textContent.includes('C final translation')));
          compact(2);await wait(()=>cards().length===2&&!cards().some(card=>card.textContent.includes('Finished C')));
          compact(4);await wait(()=>cards().length===3&&cards().some(card=>card.textContent.includes('C final translation')));
          finalC.translations=[{target_language:'zh',text:'C final translation'}];clearTranslationPartials();
          compactProps.subtitles=[finalC];compactProps.subtitleHistory=compactProps.subtitles;compact(4);
          const nextC={...preview(2,'C speaks again'),utterance_id:'player-2-next'};
          publishLivePartial(nextC);await wait(()=>cards().some(card=>card.textContent.includes('C speaks again')));
          check(cards().length===3&&!cards().some(card=>card.textContent.includes('Finished C')),'A new preview replaces only its own completed lane');
          completeLivePartial('speaker',nextC.utterance_id);await wait(()=>cards().some(card=>card.textContent.includes('C final translation')));
          for(const index of [0,1]){
            completeLivePartial('speaker',preview(index).utterance_id);
            compactProps.subtitles=[...compactProps.subtitles,{...finalC,id:1100+index,utterance_id:'finished-'+index,speaker:players[index],text:'Finished '+players[index].display_name,translations:[]}];
            compactProps.subtitleHistory=compactProps.subtitles;compact(4);
            await wait(()=>cards().length===3&&cards().some(card=>card.textContent.includes('Finished '+players[index].display_name)));
            check(cards().some(card=>card.textContent.includes('C final translation')),'Finished translations survive as other players stop speaking');
          }
          clearLivePartials();compactProps.subtitles=[];compactProps.subtitleHistory=[];
          for(let i=0;i<4;i++)publishLivePartial(preview(i));compact(4);await wait(()=>cards().length===4);
        }else{
          const messages=players.slice(0,4).map((player,index)=>({speaker:player.display_name,text:'Hello from '+player.display_name+'\n这是玩家 '+(index+1)+' 的实时译文。',side:'left'}));
          for(const dark of [false,true]){
            surface(messages,'正在获取语音密钥\n已等待 2.5 秒',dark);
            await document.fonts.ready;
            check(document.querySelectorAll('.vr-caption-card').length===4,'All players render at once');
            const lanes=[...document.querySelectorAll('.vr-caption-card')];
            check(lanes.every((card,index)=>index===0||card.getBoundingClientRect().top>=lanes[index-1].getBoundingClientRect().bottom),'VR lanes are stacked vertically');
            check([...document.querySelectorAll('.vr-headset-language span')].every(node=>node.textContent.trim().length>0),'Every VR language slot contains visible text');
            check([...document.querySelectorAll('.vr-caption-speaker')].map(node=>node.textContent).join(',')==='Alice,Bob,千夏,小明','Each VR lane has the speaker name');
            check(document.querySelector('[role="status"]').textContent.includes('语音密钥'),'VR includes key acquisition notice');
            const panel=document.querySelector(mode==='headset'?'.vr-headset-panel':'.vr-dialogue-body').getBoundingClientRect();
            check([...document.querySelectorAll('.vr-caption-card')].every(card=>{const rect=card.getBoundingClientRect();return rect.top>=panel.top&&rect.bottom<=panel.bottom+1&&rect.right<=panel.right;}),'VR cards stay within the panel');
          }
          for(const notice of ['等待 VRChat 启动','等待 VRChat 世界加载完成','正在重新获取语音密钥','获取失败：扫描超时']){
            surface([],notice);check(document.querySelector('[role="status"]').textContent===notice,'Status is visible without any subtitle');
          }
          surface(messages,null);check(!document.querySelector('[role="status"]'),'Resolved status clears without clearing captions');
          messages[1]={...messages[1],speaker:'Bob 改名',text:'Updated Bob only\n只更新 Bob'};
          surface(messages,'密钥已通过认证 · 获取耗时 2.5 秒');
          check(document.querySelectorAll('.vr-caption-card')[0].textContent.includes('Alice'),'Unrelated VR lane remains intact');
          const original=(index,language,text,live=true,source='speaker')=>({source,track_id:'player-'+index,text,language,translation_sources:[],live});
          const filtered=[
            {speaker:'Hidden player',text:'Hidden English',side:'left',original:original(0,'en','Hidden English')},
            {speaker:'Chinese player',text:'Translated only',side:'left',original:original(1,null,'今天我们准备检查中文过滤功能。')},
            {speaker:'Japanese player',text:'日本語',side:'left',original:original(2,'ja','日本語')},
            {speaker:'English player',text:'English original\n中文译文仍然显示',side:'left',original:original(3,'en','English original')},
            {speaker:'French player',text:'Bonjour',side:'left',original:original(4,'fr','Bonjour')},
            {speaker:'Me',text:'自己的中文',side:'right',original:original(5,'zh','自己的中文',true,'microphone')},
          ];
          const selection={visibility:{hidden_tracks:['player-0'],hidden_source_languages:['zh','ja']},max_visible_players:2,max_entries:3};
          surface(filtered,null,false,selection);
          const names=()=>[...document.querySelectorAll('.vr-caption-speaker')].map(node=>node.textContent).join(',');
          check(names()==='English player,French player,Me','VR filters the same original/track policy before limiting and retains self');
          const finalMessages=filtered.map(message=>({...message,original:{...message.original,live:false}}));
          surface(finalMessages,'等待 VRChat 启动',false,selection);
          check(names()==='English player,French player,Me','VR final captions use the same filtering as live previews');
          surface(filtered,null,false,{...selection,visibility:{hidden_tracks:[],hidden_source_languages:[]},max_visible_players:0});
          check(document.querySelectorAll('.vr-caption-card').length===6,'Updating VR preferences restores already buffered speech');
          surface(filtered.slice(0,3),'等待 VRChat 启动',false,selection);
          check(!document.querySelector('.vr-caption-card')&&document.querySelector('[role="status"]'),'Hiding every caption preserves capture notices');
          surface(filtered,null,false,selection);
        }
        document.getElementById('result').textContent='PASS';
      }catch(error){document.getElementById('result').textContent='FAIL: '+error.stack;}
    `.replace('MODE_PLACEHOLDER',JSON.stringify(mode));
    const bundle=await build({stdin:{resolveDir:join(testDirectory,'..'),loader:'tsx',contents:source},bundle:true,write:false,outfile:'captions.js',format:'esm',platform:'browser',jsx:'automatic',loader:{'.png':'dataurl'},define:{'import.meta.env.VITE_VRCS_SESSION_TOKEN':'""'}});
    const directory=mkdtempSync(join(testDirectory,'.player-captions-browser-'));
    try{
      const javascript=bundle.outputFiles.find(file=>file.path.endsWith('.js')).text;
      const css=bundle.outputFiles.find(file=>file.path.endsWith('.css')).text;
      const html=join(directory,'test.html');
      writeFileSync(html,'<!doctype html><style>'+css+'</style><div id="root"></div><pre id="result">PENDING</pre><script>window.addEventListener("error",e=>document.getElementById("result").textContent="FAIL: "+e.message);window.addEventListener("unhandledrejection",e=>document.getElementById("result").textContent="FAIL: "+e.reason);</script><script type="module">'+javascript.replace(/<\/script/gi,'<\\/script')+'</script>');
      const screenshot=resolve(testDirectory,'../../../core/target/player-captions-'+mode+'.png');
      assert.equal(await runBrowserFixture(html,screenshot,directory,mode.startsWith('compact')?760:1100),'PASS');
    }finally{
      assert(relative(testDirectory,directory).startsWith('.player-captions-browser-'));
      rmSync(directory,{recursive:true,force:true,maxRetries:10,retryDelay:200});
    }
  });
}
async function runBrowserFixture(html,screenshot,directory,width) {
  const child=spawn(browser,['--headless=new','--remote-debugging-port=0','--disable-gpu','--no-first-run',
    '--disable-background-networking','--disable-extensions','--window-size='+width+',1500',
    '--user-data-dir='+join(directory,'profile'),pathToFileURL(html).href],{windowsHide:true,timeout:40000});
  let socket;
  const pending=new Map();
  try {
    const endpoint=await new Promise((resolve,reject)=>{
      let stderr='';
      child.once('error',reject);
      child.once('exit',code=>reject(new Error('Browser exited: '+code+' '+stderr)));
      child.stderr.on('data',chunk=>{
        stderr+=chunk;
        const match=stderr.match(/DevTools listening on (ws:\/\/[^\s]+)/);
        if(match)resolve(match[1]);
      });
    });
    socket=new WebSocket(endpoint);
    await new Promise((resolve,reject)=>{socket.addEventListener('open',resolve,{once:true});socket.addEventListener('error',reject,{once:true});});
    socket.addEventListener('message',event=>{
      const response=JSON.parse(event.data),request=pending.get(response.id);
      if(request){pending.delete(response.id);response.error?request.reject(new Error(response.error.message)):request.resolve(response.result);}
    });
    socket.addEventListener('close',()=>{for(const request of pending.values())request.reject(new Error('Browser connection closed'));pending.clear();});
    let sequence=0;
    const send=(method,params={},sessionId)=>new Promise((resolve,reject)=>{
      const id=++sequence;pending.set(id,{resolve,reject});socket.send(JSON.stringify({id,method,params,sessionId}));
    });
    const {targetInfos}=await send('Target.getTargets');
    const {sessionId}=await send('Target.attachToTarget',{targetId:targetInfos.find(target=>target.type==='page').targetId,flatten:true});
    let report='PENDING';
    const deadline=Date.now()+30000;
    while(Date.now()<deadline){
      const result=await send('Runtime.evaluate',{expression:'document.getElementById("result")?.textContent',returnByValue:true},sessionId);
      report=result.result?.value??'PENDING';
      if(report!=='PENDING')break;
      await new Promise(resolve=>setTimeout(resolve,50));
    }
    const capture=await send('Page.captureScreenshot',{format:'png'},sessionId);
    writeFileSync(screenshot,Buffer.from(capture.data,'base64'));
    return report;
  } finally {
    socket?.close();
    child.kill();
    await new Promise(resolve=>child.exitCode!==null?resolve():child.once('exit',resolve));
  }
}
