import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const testDirectory = dirname(fileURLToPath(import.meta.url));
const browser = [process.env.PROGRAMFILES, process.env["PROGRAMFILES(X86)"], process.env.LOCALAPPDATA]
  .filter(Boolean).flatMap(root => [join(root, "Google/Chrome/Application/chrome.exe"), join(root, "Microsoft/Edge/Application/msedge.exe")]).find(existsSync);

test("compact chatbox shares sending state, respects IME, and restores native geometry", {
  timeout: 45000, skip: !browser && "Chrome or Edge is required",
}, async () => {
  const bundle = await build({
    stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: String.raw`
      import React, {useState} from 'react';
      import {createRoot} from 'react-dom/client';
      import {flushSync} from 'react-dom';
      import {mockIPC, mockWindows} from '@tauri-apps/api/mocks';
      import i18next from 'i18next';
      import {initReactI18next} from 'react-i18next';
      import locale from './src/i18n/locales/zh-CN.json';
      import {CompactEntry} from './src/app/CompactEntry';
      import {ChatboxComposer} from './src/chatbox/ChatboxComposer';
      import {useCompactWindow} from './src/shell/useCompactWindow';
      import {useChatboxWorkspace} from './src/chatbox/useChatboxWorkspace';
      import {useSubtitleStream} from './src/subtitles/hooks/useSubtitleStream';
      import {useConversationCatalog} from './src/conversations/useConversationCatalog';
      import {subtitlesForCompactView} from './src/shell/compact-mode';
      import {previewChatboxLocally} from './src/chatbox/chatbox';
      import './src/styles.css';

      const check=(value,message)=>{if(!value)throw new Error(message);};
      const wait=async(predicate)=>{const end=Date.now()+4000;while(!predicate()){if(Date.now()>end)throw new Error('Timed out: '+predicate);await new Promise(resolve=>setTimeout(resolve,20));}};
      const settle=()=>new Promise(resolve=>setTimeout(resolve,80));
      const noop=()=>{};
      let nativeSize={width:1180,height:760}, constraints, delaySize=null;
      const nativeErrors=[], sizeTrace=[];
      const applySize=size=>{
        nativeSize=size;
        sizeTrace.push({...size});
        document.documentElement.style.setProperty('--test-width',size.width+'px');
        document.documentElement.style.setProperty('--test-height',size.height+'px');
        window.dispatchEvent(new Event('resize'));
      };
      mockWindows('main');
      let legacyPreferences, finishReadingPreferences, failPreferenceRead=false;
      const savedPreferences=[];
      mockIPC(async(cmd,args)=>{
        if(cmd==='plugin:window|inner_size')return nativeSize;
        if(cmd==='plugin:window|scale_factor')return 1;
        if(cmd==='plugin:window|set_size'){
          if(delaySize)await new Promise(resolve=>{delaySize=resolve;});
          await new Promise(resolve=>setTimeout(resolve,32));
          applySize(args.value.size);return;
        }
        if(cmd==='plugin:window|set_size_constraints'){constraints=args.value;return;}
        if(legacyPreferences){
          if(cmd==='plugin:store|load')return 1;
          if(cmd==='plugin:store|get'&&args.key==='chatboxSendSettings'){
            if(failPreferenceRead)throw new Error('Native read failed');
            await new Promise(resolve=>{finishReadingPreferences=resolve;});
            return [legacyPreferences,true];
          }
          if(cmd==='plugin:store|set'){savedPreferences.push(args.value);return;}
          if(cmd==='plugin:store|save')return;
        }
        if(cmd.startsWith('plugin:store|'))throw new Error('Use browser preferences');
      });
      localStorage.clear();
      const sent=[], translated=[];
      let finishSend, finishTranslation, failSend=false;
      const recorded=[];
      const conversation={id:'active',started_at:'2026-10-26T00:00:00Z',ended_at:null,
        automatic_title:null,custom_title:null,icon:null,subtitle_count:0,updated_at:'2026-10-26T00:00:00Z',active:true};
      let socket;
      window.WebSocket=class {
        constructor(){socket=this;queueMicrotask(()=>this.onopen?.());}
        close(){}
        receive(message){this.onmessage?.({data:JSON.stringify(message)});}
      };
      const json=value=>new Response(JSON.stringify(value),{headers:{'Content-Type':'application/json'}});
      window.fetch=async(url,init)=>{
        if(url.endsWith('/api/conversations'))return json({conversations:[conversation]});
        if(url.includes('/api/conversations/active/subtitles'))return json({items:[...recorded].reverse(),has_more:false,next_before_id:null});
        const input=JSON.parse(init.body);
        if(url.endsWith('/api/chatbox/preview'))return json(previewChatboxLocally(input));
        if(url.endsWith('/api/translations/preview')){
          translated.push(input);
          await new Promise(resolve=>{finishTranslation=resolve;});
          return json({text:'Hello translated',source_language:'zh',target_language:input.target_language});
        }
        if(url.endsWith('/api/chatbox/messages')){
          sent.push(input);
          await new Promise(resolve=>{finishSend=resolve;});
          if(failSend)return new Response(JSON.stringify({detail:'Send failed'}),{status:500});
          const created_at=new Date().toISOString();
          const subtitle={id:recorded.length+1,conversation_id:'active',source:'chatbox',speaker:null,
            text:input.send_mode==='translation'?input.translation:input.original,
            language:input.send_mode==='translation'?input.target_language:input.source_language,
            started_at:null,ended_at:null,created_at,translations:input.send_mode==='bilingual'
              ?[{text:input.translation,source_language:input.source_language,target_language:input.target_language,provider:'local',model:null,created_at}]:[]};
          recorded.push(subtitle);
          socket.receive({type:'subtitle',subtitle});
          return json({...input,id:recorded.length,source:'manual',rendered_text:previewChatboxLocally(input).text,
            char_count:previewChatboxLocally(input).char_count,truncated:false,status:'sent',error_code:null,error_detail:null,
            resent_from_id:null,created_at,sent_at:created_at});
        }
        throw new Error('Unexpected fetch: '+url);
      };
      await i18next.use(initReactI18next).init({lng:'zh-CN',resources:{'zh-CN':{translation:locale.translation}}});
      const root=createRoot(document.getElementById('root'));
      let controller, chat, stream, update, flags, selectionClears=0;
      function Fixture(){
        const [state,setState]=useState({ready:true,enabled:true,osc:true,unitext:false,overlay:false,transparent:false,outputMode:'system'});
        flags=state;update=partial=>flushSync(()=>setState(current=>({...current,...partial})));
        const settings={features:{glossary:state.enabled,learning:state.enabled},osc:{enabled:state.osc},unitext:{enabled:state.unitext},translation:{microphone_targets:[{target_language:state.preferredTarget??'en'}]},audio:{output:{mode:state.outputMode}}};
        controller=useCompactWindow({clearErrorFrom:noop,reportError:reason=>nativeErrors.push(reason)});
        chat=useChatboxWorkspace(settings,state.room);
        stream=useSubtitleStream({coreConfigured:true,clearErrorFrom:noop,reportError:noop});
        useConversationCatalog({coreReady:true,conversationCatalogEvent:stream.conversationCatalogEvent,
          openConversation:stream.openConversation,openConversationAt:stream.openConversationAt,clearErrorFrom:noop,reportError:noop});
        const props={draft:chat.draft,preferredTarget:chat.preferredTarget,preview:chat.preview,busy:chat.busy,feedback:chat.feedback,translationStale:chat.translationStale,
          oscEnabled:state.osc,unitextEnabled:state.unitext,languageCodes:['en','ja'],allowCustomLanguage:true,onDraftChange:chat.setDraft,onTranslate:chat.translate,onSend:chat.send};
        return controller.compact?<CompactEntry
          compact={{...controller,transparentBackground:state.transparent,overlayOpen:state.overlay,selectionPanelOpen:false,
            subtitles:subtitlesForCompactView(stream.subtitles),subtitleHistory:stream.subtitles}}
          chatbox={chat} providers={{translationLanguageCodes:['en','ja'],allowCustomTranslationLanguage:true}}
          runtime={{ready:state.ready,reportError:noop}} capture={{pending:false,toggleCapture:noop}}
          settings={{value:settings}} selection={{target:null,clear:()=>{selectionClears++;},selectText:async()=>{}}}
          learning={{workspace:{}}} playerDisplay={{hiddenTracks:[],hiddenSourceLanguages:[]}}/>
          :chat.open?<ChatboxComposer {...props}/>:null;
      }
      const key=(element,key,options={})=>{
        const event=new KeyboardEvent('keydown',{key,bubbles:true,cancelable:true,...options});
        flushSync(()=>element.dispatchEvent(event));return event;
      };
      const input=()=>document.getElementById('chatbox-original-input');
      const toggle=()=>document.querySelector('.compact-chatbox-toggle');
      const expand=()=>document.querySelector('.chatbox-expand-button');
      const sendButton=()=>document.querySelector('.chatbox-send-button');
      const draft=partial=>flushSync(()=>chat.setDraft({...chat.draft,...partial}));
      const sendOriginal=async(original)=>{
        const count=sent.length;
        draft({original,send_mode:'original',translation:null});await wait(()=>!sendButton().disabled);key(input(),'Enter');
        await wait(()=>sent.length===count+1);finishSend();await wait(()=>chat.busy===null);await settle();
      };
      const click=element=>flushSync(()=>element.click());
      const captions=()=>document.querySelector('.compact-subtitle-stack')?.textContent??'';
      const composerSpace=()=>Math.ceil(document.querySelector('.compact-chatbox')?.getBoundingClientRect().height??0);
      const fitComposer=async(base)=>{
        await new Promise(resolve=>setTimeout(resolve,260));
        await wait(()=>nativeSize.height===Math.min(720,base+composerSpace()));
      };
      try{
        function MigrationFixture(){
          chat=useChatboxWorkspace({translation:{microphone_targets:[{target_language:'en'}]}});
          return null;
        }
        legacyPreferences={target_language:'de',send_mode:'translation',message_format:'custom',
          custom_format:'{translation} / {original}',overflow_policy:'smart_truncate'};
        flushSync(()=>root.render(<MigrationFixture/>));
        await wait(()=>finishReadingPreferences);
        flushSync(()=>chat.show());
        check(!chat.open,'Early open waits for saved preferences');
        finishReadingPreferences();await wait(()=>chat.open);
        check(!chat.draft.follow_translation_settings&&chat.draft.target_language==='de',
          'Legacy native fixed target takes precedence over the new follow default');
        check(chat.draft.send_mode==='translation'&&chat.draft.overflow_policy==='smart_truncate'
          &&!savedPreferences.length,
          'Opening preserves legacy options without writing to the store');
        draft({follow_translation_settings:true});
        await wait(()=>savedPreferences.length===1);
        check(savedPreferences[0].target_language==='de'&&savedPreferences[0].follow_translation_settings,
          'Following retains the legacy manual target on disk');
        flushSync(()=>root.render(null));
        failPreferenceRead=true;
        flushSync(()=>root.render(<MigrationFixture/>));
        flushSync(()=>chat.show());await wait(()=>chat.open);
        draft({send_mode:'original'});
        await wait(()=>JSON.parse(localStorage.getItem('vrcs.chatbox-send-settings.v1'))?.send_mode==='original');
        check(savedPreferences.length===1,'Fallback edits cannot overwrite unread native preferences');
        legacyPreferences=null;localStorage.clear();
        flushSync(()=>root.render(<Fixture/>));
        await settle();
        await controller.enterCompact();await settle();
        check(toggle(),'Compact header exposes Chatbox');
        check(!input() && nativeSize.height===160,'Composer starts closed');
        applySize({width:900,height:240});await settle();
        click(toggle());await wait(()=>input());await fitComposer(240);
        check(selectionClears===1,'Opening clears subtitle selection');
        check(document.activeElement===input(),'Opening focuses original input');
        check(nativeSize.width===900,'Opening retains resized width');
        check(constraints.minHeight.Logical===160+composerSpace(),'Composer cannot shrink away the captions');
        const captionBounds=document.querySelector('.compact-shell').getBoundingClientRect();
        check(document.querySelector('.chatbox-composer').getBoundingClientRect().top>=captionBounds.bottom,'Composer does not cover captions');
        const unusedSpace=document.querySelector('.chatbox-composer').getBoundingClientRect().bottom
          -document.querySelector('.chatbox-quick-row').getBoundingClientRect().bottom;
        const singleLineHeight=nativeSize.height;
        draft({original:'First line\nSecond line\nThird line'});await fitComposer(240);
        check(nativeSize.height>singleLineHeight&&nativeSize.height<singleLineHeight+40,'Multiline input adds only its actual height');
        draft({original:'你好',send_mode:'original'});
        await fitComposer(240);
        check(nativeSize.height===singleLineHeight,'Removing extra lines removes their reserved space');
        key(input(),'Enter',{isComposing:true,ctrlKey:true});await settle();
        check(sent.length===0,'IME Ctrl+Enter never sends');
        key(input(),'Enter',{keyCode:229});await settle();
        check(sent.length===0,'IME keyCode fallback never sends');
        check(!key(input(),'Enter',{shiftKey:true}).defaultPrevented,'Shift+Enter retains native newline behavior');
        key(input(),'Escape',{isComposing:true});check(input(),'IME Escape keeps composer open');
        const quickTop=document.querySelector('.chatbox-quick-row').getBoundingClientRect().top;
        const frames=[];
        let animationFrame;
        const recordFrame=()=>{
          frames.push({top:document.querySelector('.chatbox-quick-row').getBoundingClientRect().top,
            height:document.querySelector('.chatbox-composer').getBoundingClientRect().height});
          animationFrame=requestAnimationFrame(recordFrame);
        };
        recordFrame();click(expand());await fitComposer(240);
        cancelAnimationFrame(animationFrame);
        const regressions=[];
        if(unusedSpace>16)regressions.push('Collapsed composer leaves '+unusedSpace+'px unused below input');
        if(frames.some(frame=>Math.abs(frame.top-quickTop)>1))regressions.push('Input moves while waiting for native resize');
        if(new Set(frames.map(frame=>Math.round(frame.height))).size<3)regressions.push('Settings snap instead of expanding through intermediate heights');
        check(regressions.length===0,regressions.join('; '));
        const quickBounds=document.querySelector('.chatbox-quick-row').getBoundingClientRect();
        check(document.querySelector('.chatbox-details').getBoundingClientRect().top>=quickBounds.bottom,'Compact send settings expand below input');
        check(Math.abs(quickBounds.top-quickTop)<1,'Expanding settings keeps input in place');
        check(Boolean(document.querySelector('.chatbox-quick-row').compareDocumentPosition(document.querySelector('.chatbox-details'))&Node.DOCUMENT_POSITION_FOLLOWING),'Keyboard order follows input then settings');
        draft({send_mode:'bilingual'});
        const picker=document.querySelector('.chatbox-language-control .dropdown-trigger');
        const followLabel=picker.textContent;
        check(chat.draft.follow_translation_settings&&!picker.disabled&&followLabel.includes(locale.translation.chatbox.followTranslationSettings),
          'New profiles select Follow translation settings inside the language picker');
        click(picker);await wait(()=>document.querySelectorAll('[role="option"]').length===3);
        click(document.querySelectorAll('[role="option"]')[2]);
        const manualLabel=picker.textContent;
        check(!chat.draft.follow_translation_settings&&chat.draft.target_language==='ja'&&!manualLabel.includes(locale.translation.chatbox.followTranslationSettings),
          'Choosing a language switches to manual mode');
        click(picker);await wait(()=>document.querySelectorAll('[role="option"]').length===3);
        click(document.querySelectorAll('[role="option"]')[0]);
        check(chat.draft.follow_translation_settings&&chat.draft.target_language==='ja'&&picker.textContent===followLabel,
          'Selecting Follow preserves the manual choice and displays the current preferred language');
        update({preferredTarget:'ja'});
        check(picker.textContent.includes(locale.translation.chatbox.followTranslationSettings)&&picker.textContent.includes(manualLabel),
          'Follow option label updates live with translation settings');
        update({preferredTarget:'en'});
        check(picker.textContent===followLabel,'Follow option returns when preferred target reverts');
        draft({send_mode:'original'});
        frames.length=0;recordFrame();key(input(),'Escape');await fitComposer(240);
        cancelAnimationFrame(animationFrame);
        check(frames.every(frame=>Math.abs(frame.top-quickTop)<1),'Collapsing keeps input anchored during native resize');
        check(new Set(frames.map(frame=>Math.round(frame.height))).size>=3,'Settings collapse through intermediate heights');
        check(input()&&document.querySelector('.chatbox-details-reveal').getBoundingClientRect().height===0
          &&document.querySelector('.chatbox-details-reveal').inert,'Collapsed settings are hidden and cannot take keyboard focus');
        check(nativeSize.height===singleLineHeight,'Collapsing does not leave the expanded reserve');
        click(expand());await new Promise(resolve=>setTimeout(resolve,50));click(expand());await fitComposer(240);
        check(nativeSize.height===singleLineHeight,'Reversing an unfinished expansion returns to content height');
        key(input(),'Escape');await wait(()=>!input()&&nativeSize.height===240);await settle();
        check(document.activeElement===toggle(),'Closing restores toggle focus');
        click(toggle());await fitComposer(240);
        check(input().value==='你好','Closing preserves draft');
        key(input(),'Enter');await wait(()=>sent.length===1);
        check(input().disabled,'Pending send locks input');
        await controller.exitCompact();await settle();
        check(input().value==='你好'&&input().disabled,'Normal mode shares draft and pending send');
        key(input(),'Enter');await controller.enterCompact();await settle();
        check(sent.length===1,'Switching modes does not duplicate pending send');
        finishSend();await wait(()=>chat.busy===null);await settle();
        check(input().value===''&&toggle().getAttribute('aria-expanded')==='true','Success clears text without closing');
        await wait(()=>captions().includes('你好'));
        draft({original:'需要翻译',send_mode:'bilingual',translation:'Old translation'});
        check(!chat.translationStale,'Edited translation matches the initial preferred language');
        update({preferredTarget:'ja'});
        check(chat.translationStale,'Changing the preferred language invalidates the translation');
        key(input(),'Enter');await wait(()=>translated.length===1);
        check(translated[0].target_language==='ja','Translation uses the new preferred language');
        await controller.exitCompact();await settle();
        await controller.enterCompact();await settle();
        finishTranslation();await wait(()=>sent.length===2);
        check(sent[1].target_language==='ja'&&chat.draft.follow_translation_settings,'Sending resolves the language and retains follow mode');
        check(translated.length===1&&sent[1].translation==='Hello translated','Translation-on-send survives mode switch once');
        failSend=true;finishSend();await wait(()=>chat.busy===null);await settle();
        check(input().value==='需要翻译'&&document.querySelector('[role="status"]'),'Failure retains draft and reports feedback');
        await fitComposer(240);
        check(document.querySelector('[role="status"]').getBoundingClientRect().bottom<=document.querySelector('.chatbox-composer').getBoundingClientRect().bottom,'Feedback adds space instead of being clipped');
        update({overlay:true});await wait(()=>nativeSize.height===520);check(!input(),'Overlay temporarily hides composer');
        update({overlay:false});await wait(()=>input());await fitComposer(240);
        check(input().value==='需要翻译'&&document.querySelector('[role="status"]'),'Overlay restores draft and feedback');
        update({osc:false,unitext:false});check(sendButton().disabled&&!input().disabled,'OSC and UniText off permits composing, not sending');
        update({osc:false,unitext:true});check(!sendButton().disabled&&!input().disabled,'UniText alone allows sending');
        check(!document.querySelector('[aria-label="'+locale.translation.chatbox.outputTarget+'"]'),'Output target selector is hidden when only UniText is enabled');
        update({unitext:false});
        click(toggle());await wait(()=>nativeSize.height===240);
        update({ready:false});check(toggle().disabled,'Core unready disables entry');
        update({ready:true,enabled:false});check(toggle()&&!toggle().disabled,'Disabling optional features keeps Chatbox available');
        update({enabled:true,osc:true,transparent:true});click(toggle());await fitComposer(240);
        applySize({width:880,height:320+composerSpace()});await settle();click(toggle());await wait(()=>nativeSize.height===320);
        check(nativeSize.width===880,'Closing after manual resize preserves width and subtitle height');
        click(toggle());await fitComposer(320);
        await controller.ensureCaptionHeight(400);await fitComposer(400);
        const captionHeight=nativeSize.height;
        await controller.ensureCaptionHeight(400);await settle();check(nativeSize.height===captionHeight,'Caption growth never adds composer twice');
        const cappedFrames=[];
        const recordCap=()=>{
          cappedFrames.push(document.querySelector('.chatbox-quick-row').getBoundingClientRect().top);
          animationFrame=requestAnimationFrame(recordCap);
        };
        recordCap();
        click(expand());await wait(()=>nativeSize.height===720);
        await fitComposer(400);cancelAnimationFrame(animationFrame);
        check(cappedFrames.every((top,index)=>index===0||top<=cappedFrames[index-1]+1),'Height cap smoothly reduces caption space without bouncing');
        update({overlay:true});await wait(()=>nativeSize.height===520);
        update({overlay:false});await wait(()=>nativeSize.height===720);
        key(input(),'Escape');await fitComposer(400);
        key(input(),'Escape');await wait(()=>nativeSize.height===400);
        check(controller.height===400,'Height cap and overlay retain subtitle restore height');
        delaySize=true;click(toggle());await wait(()=>typeof delaySize==='function');
        update({overlay:true});update({overlay:false});click(toggle());
        const release=delaySize;delaySize=null;release();await wait(()=>nativeSize.height===400);
        check(!input(),'Rapid open/overlay/close settles on latest state');
        click(toggle());await fitComposer(400);
        click(expand());await wait(()=>nativeSize.height===720);
        for(const scale of [0.75,1,1.5]){
          document.documentElement.style.setProperty('--interface-scale',scale);
          document.documentElement.style.setProperty('--interface-scale-inverse',1/scale);
          applySize({width:480,height:720});await settle();
          const bounds=document.querySelector('.compact-root').getBoundingClientRect();
          const composer=document.querySelector('.chatbox-composer').getBoundingClientRect();
          const button=sendButton().getBoundingClientRect();
          check(composer.width>400&&composer.right<=bounds.right&&composer.bottom<=bounds.bottom,'Composer fits minimum width at scale '+scale);
          check(button.top>=composer.top&&button.bottom<=composer.bottom,'Send control remains visible at scale '+scale);
          check(getComputedStyle(document.querySelector('.chatbox-composer')).backgroundColor!=='rgba(0, 0, 0, 0)','Transparent captions retain readable composer surface');
        }
        update({osc:true,unitext:true});
        const targetGroup=document.querySelector('[aria-label="'+locale.translation.chatbox.outputTarget+'"]');
        const modeGroup=document.querySelector('[aria-label="'+locale.translation.chatbox.sendMode+'"]');
        check(targetGroup&&modeGroup&&(targetGroup.compareDocumentPosition(modeGroup)&Node.DOCUMENT_POSITION_FOLLOWING),'Output target selector appears to the left of send mode when both are enabled');
        click(targetGroup.querySelectorAll('button')[1]);
        check(chat.draft.output_target==='unitext'&&chat.preview.limit===null,'Selecting UniText lifts the 144-character preview limit');
        click(targetGroup.querySelectorAll('button')[2]);
        check(chat.draft.output_target==='both'&&chat.preview.limit===144,'Selecting Both restores the 144-character preview limit');
        update({unitext:false});
        document.documentElement.style.setProperty('--interface-scale',1);
        document.documentElement.style.setProperty('--interface-scale-inverse',1);
        key(input(),'Escape');await fitComposer(400);
        applySize({width:720,height:160+composerSpace()});await settle();
        const preview={type:'partial',source:'microphone',utterance_id:'microphone-before-chatbox',
          text:'Ongoing microphone preview. '.repeat(50),language:'en'};
        socket.receive(preview);await wait(()=>captions().includes('Ongoing microphone'));
        failSend=false;draft({original:'Manual message after speech',send_mode:'original',translation:null});
        key(input(),'Enter');await wait(()=>sent.length===3);finishSend();await wait(()=>chat.busy===null);
        check(stream.subtitles[0]?.text==='Manual message after speech','Successful send reaches conversation history');
        await settle();
        check(captions().includes('Manual message after speech'),'New Chatbox message must remain visible over an older live preview');
        check(document.querySelector('.compact-subtitle-current .compact-speaker').textContent===locale.translation.chatbox.title,
          'Manual messages are labelled Chatbox instead of another speaker');
        socket.receive({...preview,text:preview.text+'Still the same utterance.'});await settle();
        check(captions().includes('Manual message after speech'),'Updates to an older utterance do not immediately hide the sent message');
        await controller.exitCompact();await settle();await controller.enterCompact();await settle();
        check(captions().includes('Manual message after speech'),'Remounting compact mode retains message order');
        socket.receive({...preview,utterance_id:'microphone-after-chatbox',text:'New speech after the message. '.repeat(50)});
        await wait(()=>captions().includes('New speech after'));
        const speech=document.querySelector('.compact-subtitle-stack .compact-subtitle-row:last-child');
        const speechViewport=document.querySelector('.compact-content').getBoundingClientRect();
        check(speech.textContent.includes('New speech after')&&speech.getBoundingClientRect().bottom<=speechViewport.bottom+1,
          'A later utterance becomes the visible latest caption');
        draft({original:'Bilingual message',send_mode:'bilingual',translation:null});
        key(input(),'Enter');await wait(()=>translated.length===2);finishTranslation();
        await wait(()=>sent.length===4);finishSend();await wait(()=>chat.busy===null);
        await wait(()=>captions().includes('Bilingual message')&&captions().includes('Hello translated'));
        socket.receive({type:'subtitle',subtitle:recorded.at(-1)});await settle();
        check(document.querySelectorAll('.compact-subtitle-current').length>=1
          &&[...document.querySelectorAll('.compact-subtitle-stack .compact-original')].filter(node=>node.textContent==='Bilingual message').length===1,
          'Repeated delivery does not duplicate the sent caption');
        draft({original:'Translation-only message',send_mode:'translation',translation:null});
        key(input(),'Enter');await wait(()=>translated.length===3);finishTranslation();
        await wait(()=>sent.length===5);finishSend();await wait(()=>chat.busy===null);await settle();
        check(document.querySelector('.compact-subtitle-current .compact-original').textContent==='Hello translated',
          'Translation-only sends display the delivered translation');
        socket.receive({type:'recognition_reset'});
        applySize({width:720,height:400+composerSpace()});await settle();
        for(const outputMode of ['system','vrchat_players']){
          update({outputMode});await settle();
          await sendOriginal(outputMode+' first message');
          await sendOriginal(outputMode+' second message');
          const rows=[...document.querySelectorAll('.compact-subtitle-stack .compact-subtitle-row')];
          const first=rows.find(row=>row.querySelector('.compact-original')?.textContent===outputMode+' first message');
          const second=rows.find(row=>row.querySelector('.compact-original')?.textContent===outputMode+' second message');
          check(first&&second,'Consecutive sends remain separate captions in '+outputMode);
          check(second.getBoundingClientRect().top>=first.getBoundingClientRect().bottom,
            'Each Chatbox message is appended below the previous message in '+outputMode);
          const viewport=document.querySelector('.compact-content').getBoundingClientRect();
          check(first.getBoundingClientRect().top>=viewport.top&&second.getBoundingClientRect().bottom<=viewport.bottom+1,
            'Recent Chatbox messages are visible without manual scrolling in '+outputMode);
          check(controller.height===400,'Sending messages preserves the caption window height in '+outputMode);
        }
        const players=[0,1].map(index=>({type:'partial',source:'speaker',utterance_id:'player-live-'+index,
          text:'Player '+index+' speaking',language:'en',translation:'Player '+index+' translation',target_language:'zh',
          speaker:{id:'player-'+index,index,display_name:'Player '+index}}));
        for(const player of players)socket.receive(player);
        await wait(()=>captions().includes('Player 0 speaking')&&captions().includes('Player 1 speaking'));
        for(let index=1;index<=12;index++)await sendOriginal('Tail message '+index);
        const tailRows=[...document.querySelectorAll('.compact-subtitle-stack .compact-subtitle-row')];
        check(tailRows.at(-2).textContent.includes('Tail message 11')&&tailRows.at(-1).textContent.includes('Tail message 12'),
          'The latest consecutive sends stay in order below the player captions');
        const tailViewport=document.querySelector('.compact-content').getBoundingClientRect();
        const oldestTail=tailRows.find(row=>row.querySelector('.compact-original')?.textContent==='Tail message 1');
        check(!oldestTail||oldestTail.getBoundingClientRect().bottom<=tailViewport.top,
          'Old messages scroll out of view when caption space fills');
        check(tailRows.slice(-2).every(row=>row.getBoundingClientRect().top>=tailViewport.top&&row.getBoundingClientRect().bottom<=tailViewport.bottom+1),
          'The latest Chatbox messages remain visible after the history overflows');
        check(controller.height===400,'Chatbox history does not repeatedly enlarge the native window');
        const tailStack=document.querySelector('.compact-subtitle-stack');
        tailStack.scrollTop=0;await settle();
        socket.receive({...players[0],text:'Player 0 updated'});await wait(()=>captions().includes('Player 0 updated'));
        check(captions().includes('Player 1 speaking')&&captions().includes('Tail message 11')&&captions().includes('Tail message 12'),
          'Updating a player preserves the other player and independent Chatbox messages');
        check(tailStack.scrollTop===0&&tailRows[0].getBoundingClientRect().top>=tailViewport.top,
          'Player updates do not interrupt reading earlier captions');
        for(const index of [2,3])socket.receive({...players[0],utterance_id:'player-live-'+index,
          text:'Player '+index+' speaking',speaker:{id:'player-'+index,index,display_name:'Player '+index}});
        draft({send_mode:'bilingual'});
        click(expand());await wait(()=>nativeSize.height===720);await settle();
        await wait(()=>{const stack=document.querySelector('.compact-subtitle-stack');return stack.scrollHeight>stack.clientHeight;});
        draft({original:'Visible below all players',translation:'Translated',send_mode:'bilingual'});
        const countBeforeVisible=sent.length;
        key(input(),'Enter');await wait(()=>sent.length===countBeforeVisible+1);finishSend();await wait(()=>chat.busy===null);await settle();
        const newest=document.querySelector('.compact-subtitle-stack .compact-subtitle-row:last-child').getBoundingClientRect();
        const cappedViewport=document.querySelector('.compact-content').getBoundingClientRect();
        check(newest.top>=cappedViewport.top&&newest.bottom<=cappedViewport.bottom+1,
          'A newly sent message stays visible when player lanes exceed the capped caption space');
        socket.receive({type:'recognition_reset'});key(input(),'Escape');await fitComposer(controller.height);
        for(const outputMode of ['system','vrchat_players']){
          update({outputMode});applySize({width:720,height:160+composerSpace()});await wait(()=>controller.height===160);await settle();
          const sendBilingual=async(original)=>{
            const count=sent.length,translationCount=translated.length;
            draft({original,send_mode:'bilingual',translation:null});key(input(),'Enter');
            await wait(()=>translated.length===translationCount+1);finishTranslation();
            await wait(()=>sent.length===count+1);finishSend();await wait(()=>chat.busy===null);await settle();
          };
          const firstText=outputMode+' first default-height message';
          await sendBilingual(firstText);
          const first=[...document.querySelectorAll('.compact-subtitle-stack .compact-subtitle-row')]
            .find(row=>row.querySelector('.compact-original')?.textContent===firstText);
          await sendBilingual(outputMode+' second default-height message');
          const stack=document.querySelector('.compact-subtitle-stack');
          const second=stack.querySelector('.compact-subtitle-row:last-child');
          check(first?.isConnected&&first.querySelector('.compact-original').textContent===firstText,
            'The previous bilingual message remains an independent row at default height in '+outputMode);
          check(second!==first&&second.getBoundingClientRect().top>=first.getBoundingClientRect().bottom,
            'The next bilingual message flows below the previous row at default height in '+outputMode);
          const viewport=stack.getBoundingClientRect();
          check(first.getBoundingClientRect().bottom>viewport.top&&second.getBoundingClientRect().bottom<=viewport.bottom+1,
            'The default-height viewport shows the previous message tail above the latest message in '+outputMode);
          check(controller.height===160,'Appending bilingual history does not enlarge the compact window');
          stack.scrollTop=0;await settle();
          const oldest=stack.firstElementChild.getBoundingClientRect();
          check(stack.scrollTop===0&&oldest.top>=viewport.top&&oldest.bottom<=viewport.bottom+1,
            'Earlier Chatbox rows can be read by scrolling instead of being discarded in '+outputMode);
          stack.scrollTop=stack.scrollHeight;await settle();
        }
        const sentBeforeChange=sent.length;
        draft({original:'Keep this draft',translation:null,send_mode:'bilingual'});
        key(input(),'Enter');await wait(()=>chat.busy==='translate');
        update({preferredTarget:'fr'});finishTranslation();await wait(()=>chat.busy===null);
        check(sent.length===sentBeforeChange&&chat.draft.original==='Keep this draft'&&chat.draft.translation===null
          &&chat.feedback?.text===locale.translation.chatbox.translationContextChanged,
          'Changing target mid-send keeps the draft and explains why nothing was sent');
        const previousCount=translated.length;
        void chat.translate();await wait(()=>translated.length>previousCount);
        update({room:'new-room'});finishTranslation();await wait(()=>chat.busy===null);
        check(chat.draft.translation===null&&chat.feedback?.text===locale.translation.chatbox.translationContextChanged,
          'A late translation from the previous room cannot populate the new room');
        draft({translation:'Bonjour',send_mode:'original'});update({preferredTarget:'en'});
        key(input(),'Enter');await wait(()=>sent.length===sentBeforeChange+1);
        check(sent.at(-1).translation===null&&sent.at(-1).target_language==='en'
          &&!('follow_translation_settings' in sent.at(-1)),
          'Original-only sends omit stale translation metadata and UI preferences');
        finishSend();await wait(()=>chat.busy===null);
        check(nativeErrors.length===0,'Native geometry transitions produce no errors');
        document.getElementById('result').textContent='PASS';
      }catch(error){document.getElementById('result').textContent='FAIL: '+error.stack+'\nGeometry: '+JSON.stringify({nativeSize,base:controller?.height,space:composerSpace(),sizes:sizeTrace.slice(-10)});}
    ` },
    bundle: true, write: false, outfile: "compact-chatbox.js", format: "esm", platform: "browser", jsx: "automatic",
    loader: { ".png": "dataurl" }, define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
  });
  const directory = mkdtempSync(join(testDirectory, ".compact-chatbox-browser-"));
  try {
    const html = join(directory, "test.html");
    const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
    const stylesheet = bundle.outputFiles.find(file => file.path.endsWith(".css")).text;
    writeFileSync(html, '<!doctype html><style>' + stylesheet + '.compact-root{width:var(--test-width);height:var(--test-height)}</style><div id="root"></div><pre id="result">PENDING</pre><script>globalThis.isTauri=true;window.addEventListener("error",e=>document.getElementById("result").textContent="FAIL: "+e.message);window.addEventListener("unhandledrejection",e=>document.getElementById("result").textContent="FAIL: "+e.reason);</script><script type="module">' + javascript.replace(/<\/script/gi, '<\\/script') + '</script>');
    assert.equal(await runBrowserFixture(html, directory), "PASS");
  } finally {
    assert(relative(testDirectory, directory).startsWith(".compact-chatbox-browser-"));
    rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
});

async function runBrowserFixture(html, directory) {
  const child = spawn(browser, ['--headless=new', '--remote-debugging-port=0', '--disable-gpu', '--no-first-run',
    '--disable-background-networking', '--disable-extensions', '--window-size=1100,1000',
    '--user-data-dir=' + join(directory, 'profile'), pathToFileURL(html).href], { windowsHide: true, timeout: 40000 });
  let socket;
  const pending = new Map();
  try {
    const endpoint = await new Promise((resolve, reject) => {
      let stderr = '';
      child.once('error', reject);
      child.once('exit', code => reject(new Error('Browser exited: ' + code + ' ' + stderr)));
      child.stderr.on('data', chunk => {
        stderr += chunk;
        const match = stderr.match(/DevTools listening on (ws:\/\/[^\s]+)/);
        if (match) resolve(match[1]);
      });
    });
    socket = new WebSocket(endpoint);
    await new Promise((resolve, reject) => { socket.addEventListener('open', resolve, { once: true }); socket.addEventListener('error', reject, { once: true }); });
    socket.addEventListener('message', event => {
      const response = JSON.parse(event.data), request = pending.get(response.id);
      if (request) { pending.delete(response.id); response.error ? request.reject(new Error(response.error.message)) : request.resolve(response.result); }
    });
    socket.addEventListener('close', () => { for (const request of pending.values()) request.reject(new Error('Browser connection closed')); pending.clear(); });
    let sequence = 0;
    const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
      const id = ++sequence; pending.set(id, { resolve, reject }); socket.send(JSON.stringify({ id, method, params, sessionId }));
    });
    const { targetInfos } = await send('Target.getTargets');
    const { sessionId } = await send('Target.attachToTarget', { targetId: targetInfos.find(target => target.type === 'page').targetId, flatten: true });
    let report = 'PENDING';
    const deadline = Date.now() + 30000;
    while (Date.now() < deadline) {
      const result = await send('Runtime.evaluate', { expression: 'document.getElementById("result")?.textContent', returnByValue: true }, sessionId);
      report = result.result?.value ?? 'PENDING';
      if (report !== 'PENDING') break;
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    const capture = await send('Page.captureScreenshot', { format: 'png' }, sessionId);
    writeFileSync(resolve(testDirectory, '../../../core/target/compact-chatbox.png'), Buffer.from(capture.data, 'base64'));
    return report;
  } finally {
    socket?.close();
    child.kill();
    await new Promise(resolve => child.exitCode !== null ? resolve() : child.once('exit', resolve));
  }
}
