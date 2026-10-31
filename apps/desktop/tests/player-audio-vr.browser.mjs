import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { promisify } from "node:util";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const testDirectory = dirname(fileURLToPath(import.meta.url));
const browser = [process.env.PROGRAMFILES, process.env["PROGRAMFILES(X86)"], process.env.LOCALAPPDATA]
  .filter(Boolean).flatMap(root => [join(root, "Google/Chrome/Application/chrome.exe"), join(root, "Microsoft/Edge/Application/msedge.exe")]).find(existsSync);

test("VR consent shares the pending start, enforces the reading gate and publishes actual preparation", {
  timeout: 45000, skip: !browser && "Chrome or Edge is required",
}, async () => {
  const bundle = await build({
    stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
      import React, { useState } from 'react';
      import { createRoot } from 'react-dom/client';
      import { flushSync } from 'react-dom';
      import i18next from 'i18next';
      import { initReactI18next } from 'react-i18next';
      import en from './src/i18n/locales/en-US.json';
      import zh from './src/i18n/locales/zh-CN.json';
      import hant from './src/i18n/locales/zh-Hant.json';
      import ja from './src/i18n/locales/ja-JP.json';
      import { usePlayerAudioStart } from './src/capture/usePlayerAudioStart';
      import { useVrControlBridge } from './src/vr-controls/useVrControlBridge';
      import { playerAudioPrompt } from './src/capture/player-audio-vr';
      import { SurfaceHost as Host } from './src/vr-surface/SurfaceHost';
      import { connectFixture } from 'fixture:surface';
      const SurfaceHost = ({ surface }) => <Host surface={surface} connect={connectFixture} />;
      import { VrSurface } from './src/vr-surface/VrSurface';
      import { testing, emit } from 'fixture:native';

      const resources = Object.fromEntries([en, zh, hant, ja].map(locale => [locale._meta.locale, { translation: locale.translation }]));
      await i18next.use(initReactI18next).init({ lng: 'zh-CN', resources });
      const settings = { features: { vr_overlay: true, microphone_speaker_filter: false },
        vr_overlay: { enabled: true, wrist: { enabled: true }, headset: { enabled: true } },
        osc: { enabled: false }, audio: { output: { mode: 'vrchat_players' } } };
      const passed = [];
      const check = (condition, message) => { if (!condition) throw Error(message); passed.push(message); };
      const frames = new Map();
      let frameId = 0;
      window.requestAnimationFrame = callback => { frames.set(++frameId, callback); return frameId; };
      window.cancelAnimationFrame = id => frames.delete(id);
      const frame = () => {
        const pending = [...frames.values()]; frames.clear();
        flushSync(() => pending.forEach(callback => callback(performance.now())));
      };
      const sleep = async ms => {
        await new Promise(resolve => setTimeout(resolve, ms));
        frame(); await Promise.resolve(); frame(); await Promise.resolve();
      };
      const wait = async predicate => { for (let i = 0; i < 300; i++) { if (predicate()) return; await sleep(20); } throw Error('Timed out: ' + predicate); };
      let gate, setHealth, sequence = 0;
      const bridge = createRoot(document.getElementById('bridge'));
      const surface = createRoot(document.getElementById('root'));
      function Bridge() {
        gate = usePlayerAudioStart();
        const [health, update] = useState({ requested: false, status: null });
        setHealth = update;
        useVrControlBridge({ settings, getSettings: () => settings, update: async apply => apply(settings),
          available: true, requested: health.requested, getRequested: () => health.requested,
          running: false, busy: false, setCaptureRunning: () => gate.prepare(),
          playerAudioStart: gate, playerAudioStatus: health.status });
        return null;
      }
      const request = () => emit('vr-overlay-control-request', { owner_epoch: 7, request_id: ++sequence,
        base_revision: testing.control.revision, source: 'wrist_menu', action: { set_capture_running: true } });
      const decision = (accepted, id = gate.notice.requestId, epoch = 7) => emit('vr-overlay-player-audio-decision', { owner_epoch: epoch, request_id: id, accepted });
      const viewport = () => document.querySelector('[data-vr-page-viewport]');
      let key = { generation: 1, revision: 0 };
      const show = (kind, notice = testing.audio.notice) => {
        key = { ...key, revision: key.revision + 1 };
        const view = kind === 'dashboard' ? { kind, dark: true, dashboard: { player_audio_notice: notice }, ui: {} }
          : kind === 'ocr_progress' ? { kind, dark: true, progress: testing.audio.progress } : { kind, dark: true, notice };
        return { key, hovered: null, pressed: null, pointer: null, view };
      };
      async function host(kind) {
        flushSync(() => surface.render(<SurfaceHost key={kind} surface={kind} />));
        await sleep(40);
        flushSync(() => emit('vr-surface-view', show(kind)));
        await sleep(40);
      }
      async function run() {
        flushSync(() => bridge.render(<Bridge />));
        await wait(() => testing.audio && testing.control);
        check(testing.audio.progress === null, 'Background memory inspection does not show preparation');
        request();
        await wait(() => testing.audio.notice);
        const firstId = gate.notice.requestId;
        decision(true);
        await sleep(50);
        check(gate.notice && !testing.results.length && !testing.authorize, 'VR cannot bypass the three-second gate');
        await host('player_audio');
        check(document.querySelectorAll('h2').length === 5 && document.querySelector('[data-vr-region="1"]').disabled,
          'Independent overlay contains all five sections with a disabled confirm button');
        check(testing.layout.regions.every(region => region.id >= 2), 'Disabled decisions have no native hit regions');
        emit('vr-surface-scroll', { key, delta: 0.85 });
        await wait(() => viewport().scrollTop > 0);
        const top = viewport().scrollTop;
        check(top > 0, 'VR scroll events move the explanation');
        emit('vr-surface-scroll', { key: { generation: 0, revision: 0 }, delta: 1 });
        await sleep(40);
        check(viewport().scrollTop === top, 'Stale scroll events cannot move a new explanation');
        await wait(() => testing.audio.notice.remaining_seconds === 0);
        decision(true, firstId + 1);
        decision(true, firstId, 6);
        await sleep(30);
        check(!testing.results.length && !testing.authorize, 'Wrong request IDs and old owners cannot accept consent');
        gate.decide(false, firstId);
        await wait(() => testing.results.length === 1 && testing.audio.notice === null);
        check(testing.results[0].error !== null, 'Desktop cancellation resolves the same pending VR start');

        request();
        await wait(() => testing.audio.notice);
        check(gate.notice.requestId !== firstId, 'A later start gets a fresh consent identity');
        await wait(() => testing.audio.notice.remaining_seconds === 0);
        await host('dashboard');
        check(testing.layout.regions.length === 4 && testing.layout.regions.every(region => region.dashboard?.type === 'player_audio'),
          'Dashboard mirrors consent using its native command regions');
        const confirm = testing.layout.regions.find(region => region.dashboard.value.action === 'confirm').dashboard.value;
        check(confirm.request_id === gate.notice.requestId, 'Dashboard confirmation carries the current request identity');
        decision(true, confirm.request_id);
        await wait(() => testing.authorize && testing.audio.progress?.stages[0] === 'active');
        check(testing.results.length === 1 && testing.audio.notice === null, 'Consent resolves while capture remains pending for privileges');
        testing.holdMemory = true;
        testing.authorize('ready');
        await wait(() => testing.memory && testing.audio.progress?.status === i18next.t('warnings.playerAudio.vrPreparation.memory'));
        check(testing.audio.progress.stages[0] === 'active', 'Memory checks publish an actual preparation stage');
        testing.memory({ supported: true, elevated: true, enabled: false, restart_required: false });
        await wait(() => testing.results.length === 2 && testing.audio.progress === null);
        check(testing.results[1].error === null, 'VR acceptance completes the original pending start');

        const status = { phase: 'acquiring_key', detail: null, captured_packets: 0, decrypted_packets: 0, pcm_frames: 0,
          active_players: 0, dropped_frames: 0, decode_errors: 0, key_scan: { stage: 'scanning', scanned_bytes: 120,
          total_bytes: 400, elapsed_ms: 2500, scan_elapsed_ms: 1000, candidates_tested: 0, workers: 2 } };
        flushSync(() => setHealth({ requested: true, status }));
        await wait(() => testing.audio.progress?.translation_fraction?.[0] === 120);
        await host('ocr_progress');
        check(document.querySelectorAll('[role="progressbar"]')[2].getAttribute('aria-valuenow') === '0.3', 'OCR progress renders the measured voice-key scan fraction');
        flushSync(() => setHealth({ requested: undefined, status: undefined }));
        await sleep(40);
        check(testing.audio.progress?.translation_fraction?.[0] === 120, 'A missing health snapshot keeps acquisition feedback visible');
        flushSync(() => setHealth({ requested: true, status: { ...status, key_scan: { ...status.key_scan, stage: 'verified' } } }));
        await wait(() => testing.audio.progress?.status === i18next.t('warnings.playerAudio.vrPreparation.verifying'));
        check(testing.audio.progress.stages[2] === 'active', 'A scanned candidate remains pending until the backend confirms it');
        const ready = { ...status, phase: 'streaming', key_scan: { ...status.key_scan, stage: 'verified' } };
        flushSync(() => setHealth({ requested: true, status: ready }));
        await wait(() => testing.audio.progress?.stages.every(stage => stage === 'complete'));
        flushSync(() => surface.render(<VrSurface state={show('ocr_progress')} />));
        check(document.querySelector('.vr-progress-panel header').textContent.includes(i18next.t('warnings.playerAudio.vrPreparation.ready')),
          'VR shows successful key acquisition without speech or subtitles');
        check([...document.querySelectorAll('[role="progressbar"]')].every(bar => bar.getAttribute('aria-valuenow') === '1'),
          'All preparation stages finish before the feedback closes');
        check(document.querySelector('.vr-progress-panel header .lucide-check'), 'Successful preparation uses a check mark');
        await sleep(900);
        flushSync(() => setHealth({ requested: true, status: { ...ready, captured_packets: 1 } }));
        await sleep(900);
        check(testing.audio.progress?.stages.every(stage => stage === 'complete'), 'Success remains visible for two seconds');
        await sleep(300);
        await wait(() => testing.audio.progress === null);
        check(testing.audio.progress === null, 'Health updates do not restart the success deadline');

        flushSync(() => setHealth({ requested: true, status }));
        await wait(() => testing.audio.progress?.stages[2] === 'active');
        flushSync(() => setHealth({ requested: true, status: ready }));
        await wait(() => testing.audio.progress?.stages.every(stage => stage === 'complete'));
        await sleep(1000);
        flushSync(() => setHealth({ requested: true, status: { ...status, phase: 'retrying_key', detail: 'Retrying' } }));
        await wait(() => testing.audio.progress?.detail === 'Retrying');
        await sleep(1100);
        check(testing.audio.progress?.stages[2] === 'active', 'A retry cancels the previous success deadline');
        flushSync(() => setHealth({ requested: true, status: ready }));
        await wait(() => testing.audio.progress?.stages.every(stage => stage === 'complete'));
        await sleep(500);
        flushSync(() => setHealth({ requested: false, status: { ...ready, phase: 'stopped' } }));
        await wait(() => testing.audio.progress === null);
        check(testing.audio.progress === null, 'Stopping capture closes success feedback immediately');
        flushSync(() => setHealth({ requested: true, status }));
        await wait(() => testing.audio.progress?.stages[2] === 'active');
        flushSync(() => setHealth({ requested: true, status: { ...ready, key_scan: { ...ready.key_scan, stage: 'cached' } } }));
        await wait(() => testing.audio.progress?.stages.every(stage => stage === 'complete'));
        await sleep(1600);
        check(testing.audio.progress?.stages.every(stage => stage === 'complete'), 'A previous start cannot close a new success message');
        await sleep(500);
        await wait(() => testing.audio.progress === null);
        check(testing.audio.progress === null, 'A verified cached key has its own success deadline');

        let layout;
        for (const language of Object.keys(resources)) {
          await i18next.changeLanguage(language);
          const notice = playerAudioPrompt({ requestId: 99, openedAt: 0 }, 5000, i18next.t.bind(i18next));
          for (const kind of ['player_audio', 'dashboard']) {
            flushSync(() => surface.render(<VrSurface state={show(kind, notice)} onLayout={value => layout = value} />));
            await sleep(40);
            const panel = document.querySelector('.vr-player-audio-panel');
            const bounds = document.querySelector('.vr-surface').getBoundingClientRect();
            const footer = panel.querySelector('footer').getBoundingClientRect();
            check(footer.bottom <= bounds.bottom && footer.left >= bounds.left && footer.right <= bounds.right && viewport().clientHeight > 200,
              language + ' ' + kind + ' keeps content and actions within the texture');
            check(layout.regions.length === 4 && layout.regions.every(region => region.bounds.every(value => value >= 0 && value <= 1)),
              language + ' ' + kind + ' publishes reachable native hit regions');
            viewport().scrollTop = viewport().scrollHeight;
            check(viewport().scrollTop + viewport().clientHeight >= viewport().scrollHeight - 1, language + ' ' + kind + ' exposes the full disclaimer');
          }
        }
        await i18next.changeLanguage('zh-CN');
        flushSync(() => surface.render(<VrSurface state={show('player_audio', playerAudioPrompt({ requestId: 100, openedAt: 0 }, 5000, i18next.t.bind(i18next)))} />));
        document.getElementById('result').textContent = JSON.stringify({ passed });
      }
      run().catch(error => { document.getElementById('result').textContent = JSON.stringify({ error: error.stack, passed }); });
    ` },
    plugins: [{ name: "native-fixture", setup(build) {
      build.onResolve({ filter: /^fixture:surface$/ }, () => ({ path: join(testDirectory, "fixtures/vr-surface-bridge.mjs") }));
      build.onResolve({ filter: /^(?:@tauri-apps\/api\/(?:core|event)|fixture:native)$/ }, () => ({ path: "native", namespace: "fixture" }));
      build.onLoad({ filter: /.*/, namespace: "fixture" }, () => ({ contents: `
        import { emitSurface, surfaceCalls, surfaceTesting } from 'fixture:surface';
        const listeners = new Map();
        export const testing = Object.assign(surfaceTesting, { results: [], holdMemory: false });
        export const isTauri = () => true;
        export async function invoke(command, args) {
          if (command === 'player_audio_pending_action') return null;
          if (command === 'player_audio_memory_status') return testing.holdMemory ? new Promise(resolve => testing.memory = resolve)
            : { supported: true, elevated: true, enabled: false, restart_required: false };
          if (command === 'player_audio_ensure_privileges') return new Promise(resolve => testing.authorize = resolve);
          if (command === 'vr_overlay_control_ready' || command === 'vr_overlay_control_update') return testing.control = { ...args.view, owner_epoch: 7, revision: 1 };
          if (command === 'vr_overlay_control_result') { testing.results.push(args); return; }
          if (command === 'vr_overlay_player_audio_update') { testing.audio = args.view; return true; }
          if (command === 'vr_surface_layout') { testing.layout = args.layout; return true; }
          return null;
        }
        export async function listen(name, handler) {
          const handlers = listeners.get(name) ?? new Set();
          listeners.set(name, handlers); handlers.add(handler);
          return () => handlers.delete(handler);
        }
        export function emit(name, payload) {
          if (name.startsWith("vr-surface-")) { emitSurface(name, payload); return; } for (const handler of listeners.get(name) ?? []) handler({ payload }); }
      ` }));
    } }],
    bundle: true, write: false, outfile: "player-audio-vr.js", format: "esm", platform: "browser", jsx: "automatic",
    minify: true, loader: { ".png": "dataurl" },
  });
  const directory = mkdtempSync(join(testDirectory, ".player-audio-vr-"));
  try {
    const javascript = bundle.outputFiles.find(file => file.path.endsWith(".js")).text;
    const css = bundle.outputFiles.find(file => file.path.endsWith(".css")).text;
    const html = join(directory, "test.html");
    writeFileSync(html, '<!doctype html><style>body{margin:0;background:#171b22}' + css + '</style><div id="bridge"></div><div id="root"></div><output id="result" style="display:none"></output><script type="module">'
      + javascript.replace(/<\/script/gi, "<\\/script") + '</script>');
    const { stdout } = await promisify(execFile)(browser, ["--headless=new", "--dump-dom", "--virtual-time-budget=25000", "--disable-gpu", "--no-first-run",
      "--disable-background-networking", "--disable-extensions", "--window-size=1100,1150",
      "--screenshot=" + resolve(testDirectory, "../../../core/target/player-audio-vr.png"),
      "--user-data-dir=" + join(directory, "profile"), pathToFileURL(html).href], { timeout: 35000, maxBuffer: 8 * 1024 * 1024, windowsHide: true });
    const output = stdout.match(/<output id="result"[^>]*>([^<]+)<\/output>/)?.[1];
    assert.ok(output, "The VR fixture must finish");
    const result = JSON.parse(output.replaceAll("&gt;", ">").replaceAll("&lt;", "<").replaceAll("&amp;", "&"));
    assert.equal(result.error, undefined, result.error);
    assert.ok(result.passed.length >= 40, JSON.stringify(result));
  } finally {
    rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
});
