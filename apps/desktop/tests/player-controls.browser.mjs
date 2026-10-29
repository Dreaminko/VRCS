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

for (const [width,reduced] of [[1100,false],[860,false],[380,false],[1100,true]]) {
  test(`track selection and hidden original languages at ${width}px (reduced: ${reduced})`, {timeout:45000,skip:!browser && "Chrome or Edge is required"}, async () => {
    const bundle=await build({
      stdin:{resolveDir:join(testDirectory,".."),loader:"tsx",contents:`
        import React, {useRef,useState} from 'react';
        import {createRoot} from 'react-dom/client';
        import {flushSync} from 'react-dom';
        import {updateReducedMotionPreference} from './src/app/app-motion';
        import i18next from 'i18next';
        import {initReactI18next} from 'react-i18next';
        import locale from './src/i18n/locales/zh-CN.json';
        import {PlayerHistoryControls} from './src/subtitles/components/PlayerHistoryControls';
        import {applyPlayerSelectionAction} from './src/subtitles/player-controls';
        import {LiveView,TopStatus} from './src/subtitles/components/SubtitleViews';
        import {WorkspaceFrame} from './src/app/components/WorkspaceFrame';
        import {useLiveConversationScroll} from './src/conversations/useLiveConversationScroll';
        import {publishSpeakerIdentity} from './src/subtitles/speaker-state';
        import {publishLivePartial,completeLivePartial} from './src/core-client/realtime-state';
        import './src/styles.css';
        await i18next.use(initReactI18next).init({lng:'zh-CN',resources:{'zh-CN':{translation:locale.translation}}});
        const alice={id:'vrc:alice',user_id:'usr_00000000-0000-4000-8000-000000000001',index:0,display_name:'Alice 玩家'}, bob={id:'vrc:bob',user_id:'usr_00000000-0000-4000-8000-000000000002',index:1,display_name:'Bob 玩家'};
        const translation=(language,source_language)=>({text:'译文-'+language,source_language,target_language:language,provider:'local',created_at:'2026-10-25T00:00:00Z',model:null});
        const rows=[alice,bob,null].map((speaker,index)=>({id:index+1,source:speaker?'speaker':'microphone',speaker,
          text:'原文-'+index,language:['ja','zh-CN','zh'][index],started_at:null,ended_at:null,created_at:'2026-10-25T00:00:00Z',translations:['zh-Hans','en','ja'].map(language=>translation(language,['ja','zh-CN','zh'][index]))}));
        let state,changeLanguages,changeRows,changeFocus,changePlayers,changeHeight,scrollApi;
        const noop=async()=>{};
        function Fixture(){
          const [preferences,setPreferences]=useState({hiddenTracks:[],hiddenSourceLanguages:['zh','en']});
          const [selected,setSelected]=useState(null);
          const [selectedNames,setSelectedNames]=useState({});
          const [players,setPlayers]=useState([alice,bob]);
          const defaults=['zh','en'];
          const [subtitles,setSubtitles]=useState(rows);
          const [focused,setFocused]=useState(null);
          const [height,setHeight]=useState(850);
          changeHeight=setHeight;
          changeFocus=setFocused;
          scrollApi=useLiveConversationScroll({page:'live',activeConversationId:'current',selectedConversationId:'current',openedConversationId:'current',loadingConversationSubtitles:false,selectedConversationUpdatedAt:String(subtitles.length),focusedSubtitleId:focused});
          const scroll=scrollApi.liveScrollRef;
          state={preferences,selected,selectedNames,subtitles};changeLanguages=languages=>setPreferences(value=>({...value,hiddenSourceLanguages:languages}));changeRows=setSubtitles;changePlayers=setPlayers;
          return <div className="app-shell live-shell sidebar-collapsed" style={{height}}><div className="window-chrome">VRCS</div><div className="app-body">
            <WorkspaceFrame page="live" scrollRef={scroll} onScroll={scrollApi.onLiveScroll} toolbar={
            <TopStatus connection="connected" health={null} settings={null} controls={<PlayerHistoryControls subtitles={subtitles} status={{players:players.map(speaker=>({speaker,active:true,selected:true}))}}
              enabled={true} selectedUserIds={selected} selectedUserNames={selectedNames} defaultLanguages={defaults} preferences={preferences} onPreferences={setPreferences}
              onSelection={async (action,speakers,current)=>{await new Promise(resolve=>setTimeout(resolve,30));const next=applyPlayerSelectionAction({audio:{output:{mode:'vrchat_players',player_audio:{selected_tracks:selected,selected_user_names:selectedNames}}}},action,speakers,current).audio.output.player_audio;setSelected(next.selected_tracks);setSelectedNames(next.selected_user_names);}} />}/>
            }>
            <LiveView subtitles={subtitles} hiddenTracks={preferences.hiddenTracks} hiddenSourceLanguages={preferences.hiddenSourceLanguages??[]}
              scrollContainerRef={scroll} running={true} hasOlder={true} loading={false} loadingOlder={false} onLoadOlder={noop} onSelect={noop} />
          </WorkspaceFrame></div>{!scrollApi.followingLiveSubtitles&&<button className="live-scroll-to-bottom" onClick={()=>scrollApi.scrollLiveViewToBottom()}>返回底部</button>}</div>;
        }
        createRoot(document.getElementById('root')).render(<Fixture/>);
        const wait=async predicate=>{for(let i=0;i<150;i++){if(predicate())return;await new Promise(r=>setTimeout(r,20));}throw Error('Timed out: '+predicate);};
        const check=(condition,message)=>{if(!condition)throw Error(message);};
        const click=element=>flushSync(()=>element.click());
        const finishMotion=async panel=>{
          for(const animation of panel?.getAnimations()??[])animation.finish();
          await wait(()=>!panel?.isConnected||!panel.getAnimations().length);
        };
        const languageChip=code=>document.querySelector('.player-history-language-list button[value="'+code+'"]');
        const addLanguage=async code=>{
          const panel=document.querySelector('.player-history-panel'),height=panel.getBoundingClientRect().height;
          const trigger=document.querySelector('.player-history-languages .dropdown-trigger');
          trigger.click();
          const list=()=>document.getElementById(trigger.getAttribute('aria-controls'));
          await wait(()=>list());
          const menu=list().closest('.dropdown-menu');
          check(menu.parentElement===document.body&&getComputedStyle(menu).position==='fixed','language options float above the settings panel');
          check(Math.abs(panel.getBoundingClientRect().height-height)<1,'opening language options does not resize the settings panel');
          const search=menu.querySelector('.dropdown-search');
          search.dispatchEvent(new PointerEvent('pointerdown',{button:0,bubbles:true}));
          Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(search,code);
          search.dispatchEvent(new Event('input',{bubbles:true}));
          const option=()=>[...list().querySelectorAll('[role=option]')].find(item=>item.querySelector('small')?.textContent.split(' · ').at(-1)===code);
          await wait(option);
          option().dispatchEvent(new PointerEvent('pointerdown',{button:0,bubbles:true}));
          option().click();
          await wait(()=>languageChip(code));
          check(document.querySelector('.player-history-trigger').getAttribute('aria-expanded')==='true','interacting with floating language options keeps the settings panel open');
        };
        const row=id=>document.querySelector('[data-subtitle-id="'+id+'"]');
        try{
          check(matchMedia('(prefers-reduced-motion: reduce)').matches===${reduced},'the system motion preference is applied');
          await wait(()=>document.querySelector('.player-history-trigger'));
          changeRows([]);
          await wait(()=>!row(1));
          await new Promise(resolve=>setTimeout(resolve,100));
          const region=document.querySelector('.app-scroll-region');
          const trigger=document.querySelector('.player-history-trigger');
          const status=document.querySelector('.top-status-row');
          const toolbar=document.querySelector('.live-toolbar');
          const triggerTop=()=>trigger.getBoundingClientRect().top;
          const statusTop=()=>status.getBoundingClientRect().top;
          const initialTriggerTop=triggerTop(),initialStatusTop=statusTop();
          const triggerRect=trigger.getBoundingClientRect();
          check(toolbar.getBoundingClientRect().top===region.getBoundingClientRect().top,'conversation content extends beneath the floating toolbar');
          check(document.elementFromPoint(region.getBoundingClientRect().left+2,triggerRect.top+triggerRect.height/2)?.closest('.app-scroll-region')===region,'empty toolbar space passes pointer input through to the conversation');
          check(document.elementFromPoint(triggerRect.left+triggerRect.width/2,triggerRect.top+triggerRect.height/2)?.closest('.player-history-trigger')===trigger,'the floating personnel button remains clickable');
          check(triggerRect.width===42&&triggerRect.height===42&&getComputedStyle(trigger).borderRadius==='50%','the personnel control is a circular button');
          check(!trigger.textContent.trim()&&trigger.getAttribute('aria-label')==='人员与译文显示','the icon-only button retains an accessible name');
          if(${width}>680){
            const core=document.querySelector('.core-summary').getBoundingClientRect();
            check(triggerRect.right<core.left&&Math.abs(triggerRect.top+triggerRect.height/2-core.top-core.height/2)<1,'the button sits immediately left of Core on the same row');
          }
          check(!document.querySelector('.player-history-panel')&&trigger.getAttribute('aria-expanded')==='false','settings are collapsed initially');
          const settled=async()=>{
            await new Promise(resolve=>setTimeout(resolve,100));
            await finishMotion(document.querySelector('.player-history-panel'));
          };
          region.scrollTop=0;
          trigger.dispatchEvent(new KeyboardEvent('keydown',{key:'Tab',bubbles:true}));
          trigger.dispatchEvent(new PointerEvent('pointerdown',{button:0,bubbles:true}));
          await settled();
          check(scrollApi.followingLiveSubtitles,'tabbing or an incomplete click on a closed panel must not pause following');
          const initialRegionTop=region.getBoundingClientRect().top;
          click(trigger);
          const motionPanel=document.querySelector('.player-history-panel');
          check(motionPanel&&!motionPanel.inert,'the panel is interactive as soon as it opens');
          if(!${reduced}){
            const entry=motionPanel.getAnimations()[0];
            check(entry,'the panel animates when opening');
            entry.pause();entry.currentTime=entry.effect.getTiming().duration/2;
            const opacity=Number(getComputedStyle(motionPanel).opacity);
            check(opacity>0.7&&opacity<1&&getComputedStyle(motionPanel).transform!=='none','opening scales and fades with non-linear deceleration');
          }else check(!motionPanel.getAnimations().length,'system reduced motion skips the opening animation');
          await finishMotion(motionPanel);
          const checkAnchor=()=>{
            const panelRect=motionPanel.getBoundingClientRect();
            const buttonRect=trigger.getBoundingClientRect();
            const scale=buttonRect.width/trigger.offsetWidth;
            const [originX,originY]=getComputedStyle(motionPanel).transformOrigin.split(' ').map(parseFloat);
            check(Math.abs(panelRect.left+originX*scale-buttonRect.left-buttonRect.width/2)<1.5
              &&Math.abs(panelRect.top+originY*scale-buttonRect.top-buttonRect.height/2)<1.5,
              'the panel expands from the trigger center, not the toolbar or window corner');
          };
          checkAnchor();
          document.body.style.zoom='1.25';
          await settled();checkAnchor();
          document.body.style.zoom='';
          await settled();checkAnchor();
          const paint=document.createElement('canvas').getContext('2d');
          for(const theme of ['light','dark']){
            document.documentElement.dataset.theme=theme;
            await settled();
            paint.clearRect(0,0,1,1);paint.fillStyle=getComputedStyle(trigger).backgroundColor;paint.fillRect(0,0,1,1);
            check(paint.getImageData(0,0,1,1).data[3]===255,'the expanded button retains an opaque base in '+theme+' mode');
          }
          document.documentElement.dataset.theme=${reduced}?'dark':'light';
          click(trigger);
          check(trigger.getAttribute('aria-expanded')==='false','dismissal updates the trigger immediately');
          if(!${reduced}){
            check(motionPanel.isConnected&&motionPanel.inert&&motionPanel.getAttribute('aria-hidden')==='true','the closing panel stays mounted but cannot receive input or accessibility focus');
            const exit=motionPanel.getAnimations()[0];
            check(exit,'the panel animates when closing');
            exit.pause();exit.currentTime=exit.effect.getTiming().duration/2;
            const opacity=Number(getComputedStyle(motionPanel).opacity);
            check(opacity>0.5&&opacity<1,'closing fades with non-linear acceleration');
            click(trigger);
            check(document.querySelector('.player-history-panel')===motionPanel&&!motionPanel.inert,'rapid reopening preserves and reactivates the panel');
            const reopen=motionPanel.getAnimations()[0];
            reopen.pause();reopen.currentTime=0;
            check(Math.abs(Number(getComputedStyle(motionPanel).opacity)-opacity)<0.02,'interrupted motion continues without an opacity jump');
            await finishMotion(motionPanel);
            click(trigger);
            await wait(()=>!motionPanel.isConnected);
          }
          check(!document.querySelector('.player-history-panel'),'the panel is removed after closing');
          updateReducedMotionPreference(true);
          click(trigger);
          check(!document.querySelector('.player-history-panel').getAnimations().length,'the app reduced-motion setting also skips opening motion');
          click(trigger);
          check(!document.querySelector('.player-history-panel'),'reduced motion dismisses without an exit delay');
          updateReducedMotionPreference(false);
          click(trigger);
          await settled();
          check(Math.abs(triggerTop()-initialTriggerTop)<1 && scrollApi.followingLiveSubtitles,'expanding controls keeps the toolbar fixed and does not pause live following');
          check(region.getBoundingClientRect().top===initialRegionTop,'the floating panel does not move the conversation');
          const expanded=document.querySelector('.player-history-panel').getBoundingClientRect();
          check(expanded.left>=0&&expanded.right<=innerWidth+1&&expanded.top>triggerRect.bottom,'settings open below the toolbar within the viewport');
          const closeButton=document.querySelector('.player-history-close');
          check(document.elementFromPoint(expanded.right-25,expanded.top+25)?.closest('.player-history-close')===closeButton,'the expanded panel is not clipped by the toolbar');
          closeButton.focus();
          closeButton.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));
          await settled();
          check(!document.querySelector('.player-history-panel')&&document.activeElement===trigger,'Escape closes settings and restores focus to the trigger');
          trigger.click();await settled();
          region.dispatchEvent(new PointerEvent('pointerdown',{button:0,bubbles:true}));await settled();
          check(!document.querySelector('.player-history-panel'),'clicking outside dismisses the panel');
          trigger.click();await settled();
          await wait(()=>region.scrollHeight-region.clientHeight-region.scrollTop<=1);
          trigger.click();await settled();
          check(region.scrollTop<=1&&scrollApi.followingLiveSubtitles&&!document.querySelector('.live-scroll-to-bottom'),'collapsing an empty conversation must remove the false return-to-bottom button');
          trigger.click();await settled();trigger.click();await settled();
          check(scrollApi.followingLiveSubtitles,'repeated expansion and collapse must restore following');

          const longRows=Array.from({length:20},(_,index)=>({...rows[index%3],id:index+100}));
          changeRows(longRows);await wait(()=>row(100));await wait(()=>region.scrollHeight-region.clientHeight-region.scrollTop<=1);
          check(region.scrollTop>100,'the conversation must have a real scroll range');
          check(Math.abs(triggerTop()-initialTriggerTop)<1&&Math.abs(statusTop()-initialStatusTop)<1,'both personnel controls and status stay fixed at the bottom of a long conversation');
          check(region.scrollHeight-region.clientHeight-region.scrollTop<=1,'new messages still follow after the panel is closed');
          region.dispatchEvent(new WheelEvent('wheel',{deltaY:-120,bubbles:true}));
          region.scrollTop=0;await settled();
          check(!scrollApi.followingLiveSubtitles,'reading earlier messages pauses following');
          check(Math.abs(triggerTop()-initialTriggerTop)<1&&Math.abs(statusTop()-initialStatusTop)<1,'both toolbars stay fixed when reading older messages');
          trigger.click();await settled();trigger.click();await settled();
          check(region.scrollTop<=1&&!scrollApi.followingLiveSubtitles&&document.querySelector('.live-scroll-to-bottom'),'collapsing a panel preserves genuine history reading and its return button');

          trigger.click();await settled();
          changeHeight(620);await settled();
          check(document.querySelector('.player-history-panel').getBoundingClientRect().bottom<=document.querySelector('.app-shell').getBoundingClientRect().bottom,'the panel fits the minimum supported window height');
          check(!document.querySelector('.player-history-languages > label input'),'hidden languages have no follow-default option');
          check(document.querySelector('.player-history-languages .dropdown-trigger'),'hidden languages use the shared searchable picker');
          changeHeight(500);await settled();
          const panel=document.querySelector('.player-history-body');
          check(panel.scrollHeight>panel.clientHeight&&region.clientHeight>=100,'short windows scroll the expanded panel internally and retain a chat viewport');
          const chatTop=region.scrollTop;
          panel.scrollTop=panel.scrollHeight;await settled();
          check(panel.scrollTop>0&&panel.scrollTop+panel.clientHeight>=panel.scrollHeight-1,'every panel control is reachable through its own scrollbar');
          const lastPlayerIsReachable=()=>{
            const button=document.querySelector('.player-history-row:last-child button');
            const rect=button.getBoundingClientRect();
            return document.elementFromPoint(rect.left+rect.width/2,rect.top+rect.height/2)?.closest('button')===button;
          };
          check(lastPlayerIsReachable(),'the last player remains visible and clickable after expanding original languages and scrolling to the bottom');
          const manyPlayers=Array.from({length:24},(_,index)=>({...bob,id:'vrc:extra-'+index,user_id:'usr_00000000-0000-4000-8000-'+String(index+10).padStart(12,'0'),display_name:'Player '+index,index}));
          changePlayers(manyPlayers);await wait(()=>document.querySelectorAll('.player-history-row').length===26);
          panel.scrollTop=panel.scrollHeight;await settled();
          check(lastPlayerIsReachable(),'scrolling the menu body reaches the last player in a long list without a separate player scrollbar');
          changePlayers([alice,bob]);await wait(()=>document.querySelectorAll('.player-history-row').length===2);
          check(Math.abs(region.scrollTop-chatTop)<1&&!scrollApi.followingLiveSubtitles,'scrolling the personnel panel does not scroll the chat or change following');
          check(toolbar.getBoundingClientRect().top===region.getBoundingClientRect().top&&toolbar.getBoundingClientRect().bottom>region.getBoundingClientRect().top,'the toolbar stays above the conversation in short windows');
          changeHeight(850);await settled();trigger.click();await settled();

          scrollApi.scrollLiveViewToBottom('auto');await settled();changeFocus(100);await settled();
          trigger.click();await settled();trigger.click();await settled();
          check(!scrollApi.followingLiveSubtitles,'panel collapse must not clear a focused search result');
          changeFocus(null);await settled();
          changeRows([]);await wait(()=>!row(100));await settled();
          region.scrollTop=0;trigger.click();await settled();
          changeRows(rows);
          await wait(()=>row(1));
          check(row(1).textContent.includes('译文-zh-Hans')&&row(1).textContent.includes('译文-en')&&row(1).textContent.includes('译文-ja'),'Japanese originals keep all translation targets, including hidden source languages');
          check(!row(2)&&row(3).textContent.includes('原文-2'),'Chinese originals and every translation are hidden only for other parties');
          const untaggedRows=[
            ['这是一段用于测试的中文','speaker',alice],['今天我们准备检查中文过滤功能','speaker',bob],
            ['同時通訳ってすごいですね','speaker',alice],['… 123 🙂','speaker',alice],
            ['这是自己的中文原文','microphone',null],['Thank you very much','speaker',alice],
            ['오늘 친구들과 이야기하고 있어요','speaker',alice],
          ].map(([text,source,speaker],index)=>({...rows[0],id:10+index,text,source,speaker,language:null,
            translations:[{...translation('zh-Hans',null),text:'无标记译文-'+index}]}));
          changeRows([...rows,...untaggedRows]);
          await wait(()=>row(12)&&row(13)&&row(14)&&row(16));
          check(!row(10)&&!row(11)&&!row(15),'missing ASR and translation language metadata must not bypass Chinese and English filters');
          check(row(12).textContent.includes('无标记译文-2')&&row(16).textContent.includes('无标记译文-6'),'Japanese and Korean originals keep Chinese translations without language metadata');
          check(row(14).textContent.includes('这是自己的中文原文'),'text inference never hides the microphone');
          changeRows(rows);
          await wait(()=>state.subtitles.length===3&&!row(12));
          check(document.querySelector('legend').textContent==='隐藏的原文语言','control describes original-language hiding');
          changeLanguages(['ja']);
          await wait(()=>!row(1)&&row(2));
          check(row(2).textContent.includes('译文-ja')&&row(3).textContent.includes('译文-en'),'selected languages hide entire matching originals, not matching translation targets');
          changeRows([...rows,{...rows[0],id:4,text:'English original',language:'en-US'}]);
          await wait(()=>row(4));
          await addLanguage('fr');
          check(state.preferences.hiddenSourceLanguages.join(',')==='ja,fr','adding a language preserves the existing selection');
          languageChip('fr').click();
          await wait(()=>!languageChip('fr'));
          await addLanguage('en');
          await wait(()=>state.preferences.hiddenSourceLanguages.includes('en')&&!row(4));
          await addLanguage('zh');
          await wait(()=>state.preferences.hiddenSourceLanguages.includes('zh')&&!row(2));
          check(languageChip('zh')&&!languageChip('zh-Hans')&&!languageChip('zh-Hant'),'Chinese script variants share one original-language option');
          check(!row(1)&&row(3),'multiple hidden languages leave the microphone visible');
          languageChip('ja').click();
          await wait(()=>state.preferences.hiddenSourceLanguages.length===2&&row(1));
          check(!row(2)&&!row(4),'both remaining hidden source languages still apply');
          languageChip('zh').click();
          await wait(()=>row(2));
          languageChip('en').click();
          await wait(()=>state.preferences.hiddenSourceLanguages.length===0&&row(4));
          check(row(1)&&row(2)&&row(3),'an empty selection hides no originals or translations');
          await addLanguage('en');
          await wait(()=>!row(4));
          changeRows(rows);
          await wait(()=>state.subtitles.length===3);
          document.querySelector('[aria-label="隐藏 Bob 玩家 的记录"]').click();
          await wait(()=>!row(2));
          check(state.subtitles.length===3 && state.selected===null,'hiding never deletes history or changes capture');
          document.querySelector('.player-history-processing .dropdown-trigger').focus();
          document.querySelector('.player-history-processing .dropdown-trigger').click();
          await wait(()=>document.querySelector('[role=option]'));
          const selectedOption=[...document.querySelectorAll('[role=option]')].find(option=>option.textContent.includes('仅翻译选定的人'));
          selectedOption.focus();selectedOption.click();
          await wait(()=>Array.isArray(state.selected) && document.querySelector('[aria-label="翻译 Alice 玩家"]'));
          document.querySelector('[aria-label="翻译 Alice 玩家"]').focus();
          document.querySelector('[aria-label="翻译 Alice 玩家"]').click();
          await wait(()=>state.selected?.length===1);
          check(trigger.getAttribute('aria-expanded')==='true','saving a focused selection keeps the settings panel open');
          check(state.selected[0]===alice.user_id,'selection stores the VRChat user ID instead of the current track ID');
          check(state.selectedNames[alice.user_id]===alice.display_name,'selection saves the display label alongside the account ID');
          check(!document.querySelector('[aria-label="翻译 Bob 玩家"]').checked,'unselected player remains excluded');
          publishSpeakerIdentity({...alice,display_name:'Alice <VRC> '+ '长名字'.repeat(15)});
          await wait(()=>row(1).textContent.includes('Alice <VRC>'));
          check(!row(1).querySelector('vrc'),'display names are literal text');
          publishLivePartial({type:'partial',source:'speaker',utterance_id:'bob-live',speaker:bob,text:'隐藏中的实时记录',translation:'Hidden translation',target_language:'en'});
          await wait(()=>document.querySelector('.conversation'));
          check(!document.querySelector('.conversation').textContent.includes('隐藏中的实时记录'),'hidden track stays hidden in previews');
          const liveText=()=>document.querySelector('.conversation').textContent;
          publishLivePartial({type:'partial',source:'speaker',utterance_id:'alice-live',speaker:alice,language:null,text:'语言暂未确定',translation:'Unknown translation',target_language:'en'});
          await wait(()=>liveText().includes('语言暂未确定'));
          publishLivePartial({type:'partial',source:'speaker',utterance_id:'alice-live',speaker:alice,language:'en-US',text:'Hidden English original',translation:'隐藏的英文译文',target_language:'zh-Hans'});
          await wait(()=>!liveText().includes('语言暂未确定'));
          check(!liveText().includes('Hidden English original')&&!liveText().includes('隐藏的英文译文'),'language metadata hides both parts of an existing live preview');
          await addLanguage('zh');
          await wait(()=>state.preferences.hiddenSourceLanguages.includes('zh'));
          publishLivePartial({type:'partial',source:'speaker',utterance_id:'alice-live',speaker:alice,language:'zh-Hant',text:'隱藏的中文原文',translation:'Hidden Chinese translation',target_language:'en'});
          await new Promise(resolve=>setTimeout(resolve,40));
          check(!liveText().includes('隱藏的中文原文')&&!liveText().includes('Hidden Chinese translation'),'Chinese source matching applies to streaming originals and translations');
          publishLivePartial({type:'partial',source:'speaker',utterance_id:'alice-live',speaker:alice,language:null,text:'这是一段用于测试的中文',translation:'这是中文过滤功能的测试译文',target_language:'zh-Hans'});
          await settled();
          check(!liveText().includes('这是一段用于测试的中文')&&!liveText().includes('这是中文过滤功能的测试译文'),'untagged live Chinese hides the original and its translation');
          publishLivePartial({type:'partial',source:'speaker',utterance_id:'alice-live',speaker:alice,language:null,text:'…',translation:'短预览',target_language:'zh-Hans',conversation_preview:{text:'今天我们准备检查中文过滤功能',translation:'完整中文译文'}});
          await settled();
          check(!liveText().includes('今天我们准备检查中文过滤功能')&&!liveText().includes('完整中文译文'),'the full native original is used when the preview tail cannot identify a language');
          publishLivePartial({type:'partial',source:'microphone',utterance_id:'own-live',language:null,text:'自己的实时中文原文',translation:'自己的实时中文译文',target_language:'zh-Hans'});
          await wait(()=>liveText().includes('自己的实时中文原文')&&liveText().includes('自己的实时中文译文'));
          completeLivePartial('microphone','own-live');
          publishLivePartial({type:'partial',source:'speaker',utterance_id:'alice-live',speaker:alice,language:null,text:'日本語のライブ',translation:'实时中文译文',target_language:'zh-Hans'});
          await wait(()=>liveText().includes('日本語のライブ')&&liveText().includes('实时中文译文'));
          completeLivePartial('speaker','alice-live');
          await wait(()=>!liveText().includes('日本語のライブ'));
          changeRows([rows[1]]);
          await wait(()=>document.querySelector('.conversation').textContent.includes('当前筛选下没有记录'));
          check(document.querySelector('.conversation-load-older button'),'pagination remains available when every loaded record is hidden');
          changeRows(rows);
          await wait(()=>row(1));
          changeRows([]);changePlayers([]);
          await wait(()=>document.querySelector('.player-history-row small')?.textContent==='已保存的选择');
          check(document.querySelector('[aria-label="翻译 Alice 玩家"]').checked,'offline account selection retains its saved name');
          const rejoined={...alice,id:'vrc:new-session:new-actor',index:42,display_name:'Alice 新名字'};
          const unknown={id:'vrc:unknown',index:43,display_name:'等待确认的人'};
          changePlayers([rejoined,bob,unknown]);
          await wait(()=>document.querySelector('[aria-label="翻译 Alice 新名字"]'));
          check(document.querySelector('[aria-label="翻译 Alice 新名字"]').checked,'saved account is selected after changing track ID and name');
          check(!document.querySelector('[aria-label="翻译 Bob 玩家"]').checked,'another account stays excluded');
          check(document.querySelector('[aria-label="翻译 等待确认的人"]').disabled,'a name without an account ID cannot be selected');
          const confirmed={...unknown,user_id:'usr_00000000-0000-4000-8000-000000000003',display_name:'身份已确认'};
          publishSpeakerIdentity(confirmed);
          await wait(()=>document.querySelector('[aria-label="翻译 身份已确认"]')&&!document.querySelector('[aria-label="翻译 身份已确认"]').disabled);
          document.querySelector('[aria-label="翻译 身份已确认"]').click();
          await wait(()=>state.selected.includes(confirmed.user_id));
          check(state.selectedNames[confirmed.user_id]===confirmed.display_name,'late identity metadata is saved immediately on selection');
          changePlayers([bob]);
          await wait(()=>document.querySelector('[aria-label="翻译 Alice 新名字"]'));
          [...document.querySelectorAll('.player-history-actions button')].find(button=>button.textContent.includes('选中当前所有人')).click();
          await wait(()=>state.selected.includes(bob.user_id));
          check(state.selected.includes(alice.user_id)&&state.selected.includes(confirmed.user_id),'selecting current players preserves offline saved selections');
          document.querySelector('[aria-label="翻译 身份已确认"]').click();
          await wait(()=>!state.selected.includes(confirmed.user_id));
          check(!state.selectedNames[confirmed.user_id]&&!document.querySelector('[aria-label="翻译 身份已确认"]'),'offline selections can be removed together with their cached name');
          changeRows([rows[0]]);await wait(()=>row(1));
          document.querySelector('[aria-label="翻译 Bob 玩家"]').click();
          await wait(()=>!state.selected.includes(bob.user_id));
          check(state.selectedNames[alice.user_id]===rejoined.display_name,'selecting with older history loaded must preserve the newer saved account name');
          changeRows([]);await wait(()=>!row(1));
          check(document.documentElement.scrollWidth<=innerWidth+1,'no horizontal overflow');
          document.getElementById('result').textContent='PASS';
        }catch(error){document.getElementById('result').textContent='FAIL: '+error.message;}
      `},
      plugins:[{name:"locale-fixture",setup(build){
        build.onResolve({filter:/i18n\/catalog$/},()=>({path:"locale",namespace:"fixture"}));
        build.onLoad({filter:/.*/,namespace:"fixture"},()=>({resolveDir:join(testDirectory,".."),contents:'import locale from "./src/i18n/locales/zh-CN.json"; export const localeCatalog=[locale]; export const supportedUiLocales=["zh-CN"]; export const localeResources={"zh-CN":{translation:locale.translation}};'}));
      }}],bundle:true,write:false,outfile:"player-controls.js",format:"esm",platform:"browser",jsx:"automatic",define:{"import.meta.env.VITE_VRCS_SESSION_TOKEN":'""'},
    });
    const directory=mkdtempSync(join(testDirectory,".player-controls-browser-"));
    try{
      const javascript=bundle.outputFiles.find(file=>file.path.endsWith('.js')).text;
      const css=bundle.outputFiles.find(file=>file.path.endsWith('.css')).text;
      writeFileSync(join(directory,'content.html'),'<style>'+css+'</style><div id="root"></div><pre id="result">PENDING</pre><script>const report=document.getElementById("result");window.addEventListener("error",e=>report.textContent="FAIL: "+e.message);window.addEventListener("unhandledrejection",e=>report.textContent="FAIL: "+e.reason);new MutationObserver(()=>parent.postMessage(report.textContent,"*")).observe(report,{childList:true});</script><script type="module">'+javascript.replace(/<\/script/gi,'<\\/script')+'</script>');
      const html=join(directory,'test.html');
      writeFileSync(html,'<iframe src="content.html" style="display:block;border:0;width:'+width+'px;height:1500px"></iframe><pre id="result">PENDING</pre><script>window.addEventListener("message",e=>document.getElementById("result").textContent=e.data);</script>');
      const screenshot=resolve(testDirectory,'../../../core/target/player-controls-ui-'+width+(reduced?'-reduced':'')+'.png');
      assert.equal(await runBrowserFixture(html,screenshot,directory,width,reduced), 'PASS');
    }finally{
      assert(relative(testDirectory,directory).startsWith('.player-controls-browser-'));
      rmSync(directory,{recursive:true,force:true,maxRetries:10,retryDelay:200});
    }
  });
}

// Virtual-time dump-dom can starve native ResizeObserver callbacks. Drive the
// actual browser clock so viewport changes and live following are both tested.
async function runBrowserFixture(html,screenshot,directory,width,reduced) {
  const child=spawn(browser,['--headless=new','--remote-debugging-port=0','--disable-gpu','--no-first-run',
    '--disable-background-networking','--disable-extensions','--window-size='+width+',1500',
    ...(reduced?['--force-prefers-reduced-motion=reduce']:[]),
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
