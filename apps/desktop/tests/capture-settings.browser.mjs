import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const testDirectory = dirname(fileURLToPath(import.meta.url));
const browser = [process.env.PROGRAMFILES, process.env["PROGRAMFILES(X86)"], process.env.LOCALAPPDATA]
  .filter(Boolean).flatMap(root => [join(root, "Google/Chrome/Application/chrome.exe"), join(root, "Microsoft/Edge/Application/msedge.exe")]).find(existsSync);

for (const width of [1100, 860, 380]) {
  test(`capture navigation, Chatbox save rollback, and platform status work at ${width}px`, {
    timeout: 45000, skip: !browser && "Chrome or Edge is required",
  }, async () => {
    const bundle = await build({
      stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
        import React, { useState } from 'react';
        import { createRoot } from 'react-dom/client';
        import { flushSync } from 'react-dom';
        import i18next from 'i18next';
        import { initReactI18next } from 'react-i18next';
        import locale from './src/i18n/locales/en-US.json';
        import { CaptureSettingsSection } from './src/settings/sections/CaptureSettingsSection';
        import { SettingsTabBar } from './src/settings/components/SettingsTabBar';
        import { visibleSettingsCategories, DEFAULT_FEATURE_SETTINGS } from './src/settings/feature-availability';
        import { useSettingsDraft } from './src/settings/hooks/useSettingsDraft';
        import { ControlTooltip } from './src/shared/ui/ControlTooltip';
        import './src/styles.css';
        await i18next.use(initReactI18next).init({lng:'en-US',resources:{'en-US':{translation:locale.translation}}});
        const initial={schema_version:37,vrchat_chatbox:{enabled:false},translation:{mode:'manual',speaker_targets:[]},
          audio:{output:{mode:'system',device_id:null,trigger_threshold_dbfs:-45,
            player_audio:{enabled:false,pagefile_drive:'D:',opus_library_path:'invalid-opus.dll',max_players:2,max_visible_players:4}},
            microphone:{mode:'disabled',device_id:null,trigger_threshold_dbfs:-45}}};
        const noop=async()=>{};
        let failSave=true, latest, saveState, switchPlatform, updateChatboxStatus, saves=0;
        function Fixture(){
          const [settings,setSettings]=useState(initial);
          const [category,setCategory]=useState('audio');
          const [supported,setSupported]=useState(true);
          const [chatboxStatus,setChatboxStatus]=useState(null);
          switchPlatform=setSupported;
          updateChatboxStatus=setChatboxStatus;
          const controller=useSettingsDraft(settings,async next=>{
            saves++;
            await new Promise(resolve=>setTimeout(resolve,30));
            if(failSave)throw Error('Chatbox save failed');
            setSettings(next);return next;
          });
          latest=controller.draft;
          saveState=controller.saveState;
          const shared={draft:controller.draft,saveState:controller.saveState,saveMessage:controller.saveMessage,
            applySettings:controller.applySettings,playerAudioStart:{memory:{status:{supported,elevated:true,enabled:true,restart_required:false}}}};
          return <><SettingsTabBar activeCategory={category} visibleCategories={visibleSettingsCategories(DEFAULT_FEATURE_SETTINGS)} onChange={setCategory}/>
            {category==='audio'&&<CaptureSettingsSection {...shared} devices={[]} devicesReady={true} deviceErrors={[]}
              outputDevices={[]} microphoneDevices={[]} microphoneRunning={false} transcriptionRunning={false}
              microphoneTestRunning={false} onRefresh={noop} onStartMicrophoneTest={noop} onStopMicrophoneTest={noop}
              chatboxStatus={chatboxStatus}
              onOpenTranslation={()=>setCategory('translation')}/>}
            {category==='translation'&&<div id="translation-destination">Translation destination</div>}
            {category==='connections'&&<div id="settings-panel-connections">Connections</div>}
            {category==='about'&&<div id="settings-panel-about">About</div>}
            <ControlTooltip />
          </>;
        }
        createRoot(document.getElementById('root')).render(<Fixture/>);
        const check=(ok,message)=>{if(!ok)throw Error(message);};
        const wait=async predicate=>{for(let i=0;i<160;i++){if(predicate())return;await new Promise(r=>setTimeout(r,15));}throw Error('Timed out: '+predicate);};
        const click=element=>flushSync(()=>element.click());
        const toggle=()=>document.querySelector('.capture-chatbox-settings [role=switch]');
        const networkToggle=()=>document.querySelector('.player-audio-settings [role=switch]');
        const text=locale.translation.settings;
        const selectPlayers=async()=>{
          click(document.querySelector('.device-group .dropdown-trigger'));
          await wait(()=>document.querySelector('[role=listbox][aria-hidden=false]'));
          click([...document.querySelectorAll('[role=option]')].find(option=>
            option.querySelector('.dropdown-option-copy > span')?.textContent===text.audio.players.title));
          await wait(()=>!document.querySelector('[role=listbox][aria-hidden=false]'));
        };
        try{
          await wait(toggle);
          check(document.querySelector('h2').textContent===text.audio.title,'capture heading is localized');
          check(toggle().getAttribute('aria-checked')==='false','Chatbox defaults off');
          check(networkToggle()?.getAttribute('aria-checked')==='false','network capture defaults off');
          check(toggle().disabled&&parseFloat(getComputedStyle(toggle().closest('.settings-toggle-row')).opacity)<1,'Chatbox is disabled and dimmed while network capture is off');
          check(!toggle().closest('[title]'),'Chatbox does not use a native tooltip');
          check(!document.querySelector('.capture-chatbox-settings').textContent.includes(text.capture.networkRequired),'The network requirement is not a permanent Chatbox note');
          flushSync(()=>toggle().dispatchEvent(new PointerEvent('pointerover',{bubbles:true,pointerType:'mouse'})));
          await wait(()=>document.querySelector('.control-tooltip[role=tooltip]')?.textContent===text.capture.networkRequired);
          const tooltipBounds=document.querySelector('.control-tooltip').getBoundingClientRect();
          check(tooltipBounds.left>=0&&tooltipBounds.right<=innerWidth&&tooltipBounds.width>0,'the custom tooltip stays within the window');
          flushSync(()=>toggle().dispatchEvent(new PointerEvent('pointerout',{bubbles:true,pointerType:'mouse',relatedTarget:document.body})));
          await wait(()=>!document.querySelector('.control-tooltip'));
          click(toggle());check(saves===0&&!latest.vrchat_chatbox.enabled,'disabled Chatbox cannot save or enable reception');
          await selectPlayers();
          await wait(()=>document.querySelector('[role=dialog]'));
          check(document.querySelector('[role=dialog]').textContent.includes(text.capture.networkRequired),'blocked player audio opens the network requirement dialog');
          check(latest.audio.output.mode==='system'&&saves===0,'blocked player audio preserves the current output');
          click(document.querySelector('[role=dialog] .primary-button'));
          await wait(()=>!document.querySelector('[role=dialog]'));
          failSave=false;click(networkToggle());
          await wait(()=>saveState==='saved'&&networkToggle().getAttribute('aria-checked')==='true');
          check(!toggle().disabled&&!latest.vrchat_chatbox.enabled,'network enable unlocks Chatbox without enabling it');
          check(!toggle().closest('[data-tooltip]'),'enabled Chatbox does not show the network requirement tooltip');
          const network=document.querySelector('.player-audio-settings');
          check(network&&document.getElementById('settings-panel-audio').lastElementChild===network,'network configuration is last in capture settings');
          check(document.getElementById('player-audio-title').textContent===text.capture.networkTitle,'network card has a shared capture title');
          const card=document.querySelector('[aria-labelledby=vrchat-chatbox-title]');
          const sibling=document.querySelector('.device-group');
          const aligned=(a,b)=>Math.abs(a-b)<1;
          const fields=['drive','opus','limit','visibleLimit'].map(key=>
            network.querySelector('[aria-label="'+text.audio.players[key]+'"]')
              .closest('.field, .range-field').getBoundingClientRect());
          const [drive,opus,limit,visibleLimit]=fields;
          if(innerWidth>680){
            check(aligned(drive.top,opus.top),'pagefile drive and Opus path share the first row');
            check(aligned(limit.top,visibleLimit.top)&&limit.top>=Math.max(drive.bottom,opus.bottom),'player limits share the second row');
            check(aligned(drive.left,limit.left)&&aligned(opus.left,visibleLimit.left),'network settings use aligned columns');
          }else{
            check(fields.every((field,index)=>aligned(field.left,drive.left)&&aligned(field.right,drive.right)
              &&(!index||field.top>=fields[index-1].bottom)),'network settings stack without overlap in narrow windows');
          }
          check(aligned(card.getBoundingClientRect().left,sibling.getBoundingClientRect().left)
            &&aligned(card.getBoundingClientRect().right,sibling.getBoundingClientRect().right),'Chatbox card aligns with audio cards');
          check(aligned(document.getElementById('vrchat-chatbox-title').getBoundingClientRect().left,
            sibling.querySelector('h3').getBoundingClientRect().left),'Chatbox title aligns with audio titles');
          check(!card.querySelector('[role=status]'),'stopped Chatbox has no status block');
          flushSync(()=>updateChatboxStatus({phase:'waiting_game',received:7,accepted:2,duplicates:4,dropped:1,detail:null}));
          check(document.body.textContent.includes('Received 7'),'Chatbox counters are visible');
          failSave=true;click(toggle());
          check(!toggle().disabled&&getComputedStyle(toggle().closest('.settings-toggle-row')).opacity==='1','Chatbox stays available without dimming while saving');
          check([...network.querySelectorAll('input, .dropdown-trigger')].every(control=>!control.disabled),'Saving Chatbox does not disable unrelated network settings');
          await wait(()=>document.querySelector('[role=alert]')?.textContent==='Chatbox save failed');
          check(toggle().getAttribute('aria-checked')==='false'&&!latest.vrchat_chatbox.enabled,'failed save rolls back the switch');
          failSave=false;click(toggle());
          await wait(()=>saveState==='saved'&&toggle().getAttribute('aria-checked')==='true');
          check(latest.audio.output.mode==='system'&&latest.audio.output.player_audio.opus_library_path==='invalid-opus.dll','Chatbox does not change audio or require Opus');
          await selectPlayers();
          await wait(()=>saveState==='saved'&&latest.audio.output.mode==='vrchat_players');
          failSave=true;click(networkToggle());
          check(toggle().disabled&&toggle().getAttribute('aria-checked')==='true'&&parseFloat(getComputedStyle(toggle().closest('.settings-toggle-row')).opacity)<1&&latest.vrchat_chatbox.enabled&&latest.audio.output.mode==='vrchat_players','network disable greys out Chatbox while preserving both dependent preferences');
          await wait(()=>saveState==='error');
          check(networkToggle().getAttribute('aria-checked')==='true'&&!toggle().disabled&&latest.vrchat_chatbox.enabled&&latest.audio.output.mode==='vrchat_players','failed network save restores the master and its consumers');
          failSave=false;click(networkToggle());
          await wait(()=>saveState==='saved'&&networkToggle().getAttribute('aria-checked')==='false');
          check(toggle().disabled&&toggle().getAttribute('aria-checked')==='true'&&latest.vrchat_chatbox.enabled&&latest.audio.output.mode==='vrchat_players','saving network disable keeps both consumers checked and selected');
          check(latest.audio.output.player_audio.pagefile_drive==='D:'&&latest.audio.output.player_audio.opus_library_path==='invalid-opus.dll'&&latest.audio.output.player_audio.max_players===2,'network disable preserves capture parameters');
          click(networkToggle());
          await wait(()=>saveState==='saved'&&networkToggle().getAttribute('aria-checked')==='true');
          check(!toggle().disabled&&toggle().getAttribute('aria-checked')==='true'&&latest.audio.output.mode==='vrchat_players','network re-enable restores both consumers without manual toggles');
          click([...document.querySelectorAll('button')].find(e=>e.textContent===text.capture.openTranslation));
          await wait(()=>document.getElementById('translation-destination'));
          const audio=document.getElementById('settings-tab-audio');
          click(audio);await wait(toggle);
          flushSync(()=>switchPlatform(false));
          check(toggle().disabled&&networkToggle().disabled&&document.body.textContent.includes(text.capture.unsupported),'unsupported platforms cannot enable network capture or reception and show the reason');
          check([...document.querySelectorAll('.player-audio-settings input, .player-audio-settings .dropdown-trigger')].every(control=>control.disabled),'Unsupported network settings remain disabled');
          const tabs=[...document.querySelectorAll('[role=tab]')];
          check(tabs.at(-1).id==='settings-tab-about'&&!document.getElementById('settings-tab-debug')
            &&!document.getElementById('settings-tab-experimental'),'About is last with no separate Debug or experimental tab');
          flushSync(()=>audio.dispatchEvent(new KeyboardEvent('keydown',{key:'End',bubbles:true,cancelable:true})));
          await wait(()=>document.getElementById('settings-panel-about'));
          check(document.activeElement?.id==='settings-tab-about','End selects and focuses the final category');
          check(saves===7,'blocked selections, navigation and unsupported state do not save settings');
          check(document.documentElement.scrollWidth<=innerWidth+1,'settings fit the narrow viewport');
          click(audio);await wait(toggle);
          flushSync(()=>{switchPlatform(true);updateChatboxStatus(null);});
          document.getElementById('result').textContent='PASS';
        }catch(error){document.getElementById('result').textContent='FAIL: '+error.message;}
      ` },
      plugins: [{ name: "locale-fixture", setup(build) {
        build.onResolve({ filter: /i18n\/catalog$/ }, () => ({ path: "locale", namespace: "fixture" }));
        build.onLoad({ filter: /.*/, namespace: "fixture" }, () => ({ resolveDir: join(testDirectory, ".."),
          contents: 'import locale from "./src/i18n/locales/en-US.json"; export const localeCatalog=[locale]; export const supportedUiLocales=["en-US"]; export const localeResources={"en-US":{translation:locale.translation}};' }));
      } }],
      bundle: true, write: false, outfile: "capture-settings.js", format: "esm", platform: "browser", jsx: "automatic",
      define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
    });
    const directory = mkdtempSync(join(testDirectory, ".capture-settings-browser-"));
    try {
      const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
      const css = bundle.outputFiles.find(file => file.path.endsWith(".css")).text;
      writeFileSync(join(directory, "content.html"), '<style>' + css + '</style><div class="settings-surface" id="root"></div><pre id="result">PENDING</pre>'
        + '<script>const report=document.getElementById("result");window.addEventListener("error",e=>report.textContent="FAIL: "+e.message);window.addEventListener("unhandledrejection",e=>report.textContent="FAIL: "+e.reason);new MutationObserver(()=>parent.postMessage(report.textContent,"*")).observe(report,{childList:true});</script>'
        + '<script type="module">' + javascript.replace(/<\/script/gi, '<\\/script') + '</script>');
      const html = join(directory, "test.html");
      writeFileSync(html, `<iframe src="content.html" style="display:block;border:0;width:${width}px;height:2300px"></iframe><pre id="result">PENDING</pre>`
        + '<script>window.addEventListener("message",e=>document.getElementById("result").textContent=e.data);</script>');
      const screenshot=resolve(testDirectory,'../../../core/target/capture-settings-'+width+'.png');
      mkdirSync(dirname(screenshot),{recursive:true});
      const result = spawnSync(browser, ["--headless=new", "--dump-dom", "--virtual-time-budget=10000", "--disable-gpu", "--no-first-run",
        "--disable-background-networking", "--disable-extensions", `--window-size=${width},2300`, "--screenshot="+screenshot, "--user-data-dir=" + join(directory, "profile"), pathToFileURL(html).href],
      { encoding: "utf8", windowsHide: true, timeout: 30000, maxBuffer: 8 * 1024 * 1024 });
      assert.ifError(result.error);
      assert.equal(result.status, 0, result.stderr);
      const report = result.stdout.match(/<pre id="result">([^<]*)<\/pre>/)?.[1];
      assert.equal(report, "PASS", report || result.stderr);
    } finally {
      assert(relative(testDirectory, directory).startsWith(".capture-settings-browser-"));
      rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
    }
  });
}
