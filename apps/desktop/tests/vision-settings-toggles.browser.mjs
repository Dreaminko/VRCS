import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
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

for (const backend of ["cloud", "local"]) {
  test(`OCR (${backend}), vision and VR Overlay switches hide settings without losing configuration`, {
    timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
  }, async () => {
    const bundle = await build({
      stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
        import React, { useState } from 'react';
        import { createRoot } from 'react-dom/client';
        import i18next from 'i18next';
        import { initReactI18next } from 'react-i18next';
        import './src/styles.css';
        import { OcrSettingsCard } from './src/settings/sections/OcrSettingsCard';
        import { VisionSettingsCard } from './src/settings/sections/VisionSettingsCard';
        import { DEFAULT_OCR_SETTINGS, DEFAULT_VR_OVERLAY_SETTINGS } from './src/settings/vr-overlay-settings';
        import { VrOverlaySettingsSection } from './src/settings/sections/VrOverlaySettingsSection';
        import { DEFAULT_VISION_SETTINGS, selectVisionProfile } from './src/settings/vision-settings';
        await i18next.use(initReactI18next).init({ lng: 'en', resources: { en: { translation: {} } } });
        window.fetch = async () => new Response(JSON.stringify({
          configured: true, stored_configured: true, environment_override: false,
          state: 'ready', downloaded_bytes: 1, total_bytes: 1, models: ['image-model'],
        }), { status: 200 });
        const profiles = [{ id: 'images', name: 'Image service', provider: 'openai',
          provider_display_name: 'OpenAI', enabled_capabilities: ['image_understanding'] }];
        let ocr, vision, overlay;
        function Harness() {
          const [ocrConfig, setOcr] = useState({ ...DEFAULT_OCR_SETTINGS, master_enabled: false,
            backend: ${JSON.stringify(backend)}, display_mode: 'wrist', targets: [] });
          const [visionConfig, setVision] = useState({ ...DEFAULT_VISION_SETTINGS, vr_enabled: false });
          const [overlayConfig, setOverlay] = useState({ vr_overlay: { ...DEFAULT_VR_OVERLAY_SETTINGS,
            headset: { ...DEFAULT_VR_OVERLAY_SETTINGS.headset, enabled: true, font_size_px: 48 },
            wrist: { ...DEFAULT_VR_OVERLAY_SETTINGS.wrist, enabled: false, font_size_px: 36 },
          } });
          const [vrAvailable, setVrAvailable] = useState(true);
          window.setVrAvailable = setVrAvailable;
          ocr = ocrConfig; vision = visionConfig; overlay = overlayConfig;
          return <>
            <VrOverlaySettingsSection draft={overlayConfig} saveState="saved" applySettings={setOverlay} />
            <div id="ocr"><OcrSettingsCard config={ocrConfig} profiles={[]} disabled={false} vrAvailable={vrAvailable}
              desktopStatus={{ shortcut_error: 'desktop_ocr.shortcut_conflict' }} runtime={{ state: 'ready', last_error: 'VR error' }}
              onChange={patch => setOcr(current => ({ ...current, ...patch }))} /></div>
            <VisionSettingsCard config={visionConfig} profiles={profiles} disabled={false} vrAvailable={vrAvailable}
              providerDefinitions={[]} recognition={{ backend: 'qwen_local_managed', active_profile_id: null, language: 'auto',
                managed_qwen: { package_id: 'qwen3-asr-0.6b-q8_0', device: 'auto' }, service_settings: {} }}
              onChange={patch => setVision(current => ({ ...current, ...patch }))}
              onSelectProfile={id => setVision(current => selectVisionProfile({ vision: current }, id).vision)} />
          </>;
        }
        // Advance focus callbacks with the virtual clock even when headless Chrome does not paint.
        window.requestAnimationFrame = callback => window.setTimeout(() => callback(performance.now()), 16);
        window.cancelAnimationFrame = id => window.clearTimeout(id);
        const root = createRoot(document.getElementById('root'));
        root.render(<Harness />);
        const check = (condition, message) => { if (!condition) throw new Error(message); };
        const settle = () => new Promise(resolve => setTimeout(resolve, 20));
        const wait = async predicate => {
          for (let attempt = 0; attempt < 200; attempt++) {
            for (const animation of document.getAnimations()) {
              if (animation.effect.getTiming().iterations !== Infinity) animation.finish();
            }
            if (predicate()) { await settle(); return; }
            await settle();
          }
          throw new Error('Timed out waiting for: ' + predicate.toString());
        };
        const checkMasterHeading = (card, master, title) => {
          const heading = card.querySelector(':scope > header');
          check(heading.contains(master), title + ': master switch is in the heading');
          check(!heading.querySelector('.settings-toggle-copy'), title + ': heading switch has no visible enable label');
          check(!card.querySelector(':scope > .settings-toggle-list'), title + ': no separate master switch row remains');
          check(master.getAttribute('aria-label'), title + ': switch retains its accessible label');
          check(getComputedStyle(heading).flexDirection === 'row', title + ': heading stays horizontal');
          const label = heading.querySelector('strong').getBoundingClientRect();
          const control = master.getBoundingClientRect();
          check(control.left >= label.right && control.right <= heading.getBoundingClientRect().right + 1, title + ': master switch is on the right');
        };
        const collapsed = (section, title) => {
          check(section.querySelectorAll('[role=switch]').length === 1, title + ': only its switch remains');
          check(!section.querySelector('input, textarea, .dropdown-trigger, details, [role=status], [role=alert]'), title + ': settings and errors are hidden');
          const toggleList = section.querySelector('.settings-toggle-list');
          if (toggleList) {
            const style = getComputedStyle(toggleList);
            check(style.borderBottomWidth === '0px', title + ': no divider remains below the switch');
            check(style.paddingBottom === '0px', title + ': no divider spacing remains below the switch');
          }
          check(!section.querySelector('hr, [role=separator]'), title + ': no orphan separator remains');
        };
        try {
          await wait(() => document.querySelector('.vision-settings-card'));
          const ocrCard = document.querySelector('#ocr > section');
          const visionCard = document.querySelector('.vision-settings-card');
          const ocrMaster = ocrCard.querySelector('[role=switch]');
          const visionMaster = visionCard.querySelector('[role=switch]');
          collapsed(ocrCard, 'OCR'); collapsed(visionCard, 'Vision');
          checkMasterHeading(ocrCard, ocrMaster, 'OCR');
          checkMasterHeading(visionCard, visionMaster, 'Vision');
          check(!ocrCard.querySelector(':scope > header .vr-overlay-status-badge'), 'OCR: model badge is removed from the heading');
          const overlayPanel = document.querySelector('#settings-panel-vr_overlay');
          const overlayMaster = overlayPanel.querySelector('[role=switch]');
          const overlaySnapshot = JSON.stringify(overlay);
          for (let cycle = 0; cycle < 2; cycle++) {
            collapsed(overlayPanel, 'VR Overlay');
            check(overlayPanel.querySelectorAll('button').length === 1, 'VR Overlay: retry and sample actions are hidden');
            check(!overlayPanel.querySelector('.vr-overlay-runtime-main, .vr-overlay-runtime-facts, .vr-overlay-card-list'), 'VR Overlay: runtime details and mode cards are hidden');
            overlayMaster.click();
            await wait(() => overlayPanel.querySelector('.vr-overlay-card-list'));
            check(overlayPanel.querySelector('.vr-overlay-runtime-main'), 'VR Overlay: runtime details are restored');
            check(overlayPanel.querySelector('.dropdown-trigger'), 'VR Overlay: translation setting is restored');
            check(overlayPanel.querySelectorAll('.vr-overlay-card').length === 2, 'VR Overlay: both mode cards are restored');
            check(overlay.vr_overlay.headset.enabled && !overlay.vr_overlay.wrist.enabled, 'VR Overlay: mode preferences are unchanged');
            for (const kind of ['headset', 'wrist']) {
              const card = overlayPanel.querySelector('[aria-labelledby=vr-overlay-' + kind + '-title]');
              const toggle = card.querySelector('[role=switch]');
              const original = overlay.vr_overlay[kind].enabled;
              const branchSnapshot = JSON.stringify(overlay.vr_overlay[kind]);
              const other = kind === 'headset' ? 'wrist' : 'headset';
              const otherSnapshot = JSON.stringify(overlay.vr_overlay[other]);
              for (const enabled of [false, true, original]) {
                if (overlay.vr_overlay[kind].enabled !== enabled) toggle.click();
                await wait(() => toggle.getAttribute('aria-checked') === String(enabled));
                check(card.querySelector('strong[id]'), kind + ': title remains visible');
                check(!toggle.disabled, kind + ': mode switch remains available');
                checkMasterHeading(card, toggle, kind);
                if (enabled) {
                  check(card.querySelector('input'), kind + ': settings are restored');
                  check(card.querySelector('.vr-overlay-status-badge'), kind + ': status is restored');
                  check(card.querySelector('.vr-overlay-card-actions'), kind + ': actions are restored');
                } else {
                  collapsed(card, kind);
                  check(card.querySelectorAll('button').length === 1, kind + ': only the mode switch remains');
                  check(!card.querySelector('.vr-overlay-status-badge, .vr-overlay-device-summary'), kind + ': status and device details are hidden');
                }
                check(JSON.stringify(overlay.vr_overlay[other]) === otherSnapshot, kind + ': other mode is unchanged');
              }
              check(JSON.stringify(overlay.vr_overlay[kind]) === branchSnapshot, kind + ': all settings are retained');
            }
            overlayMaster.click();
            await wait(() => !overlayPanel.querySelector('.vr-overlay-card-list'));
            check(JSON.stringify(overlay) === overlaySnapshot, 'VR Overlay: all configuration is retained');
          }
          check(!visionMaster.disabled, 'Unconfigured vision can be enabled');
          check(visionCard.querySelectorAll('button').length === 1, 'Disabled vision only has its master switch');
          visionMaster.click();
          await wait(() => visionCard.querySelector('.vr-overlay-field-grid'));
          check(vision.enabled && vision.profile_id === null && vision.model === '', 'Vision can be enabled with empty connection settings');
          check(!document.querySelector('[role=dialog]'), 'Vision configuration is inline without an edit dialog');
          const selectProfile = async label => {
            visionCard.querySelector('.vr-overlay-field-grid .dropdown-trigger').click();
            await wait(() => [...document.querySelectorAll('[role=option]')].some(option => option.textContent.includes(label)));
            [...document.querySelectorAll('[role=option]')].find(option => option.textContent.includes(label)).click();
            await settle();
            await new Promise(requestAnimationFrame);
          };
          const selectModel = async label => {
            await wait(() => !visionCard.querySelector('.translation-model-input-row .editable-dropdown-toggle').disabled);
            visionCard.querySelector('.translation-model-input-row .editable-dropdown-toggle').click();
            await wait(() => [...document.querySelectorAll('[role=option]')].some(option => option.textContent === label));
            [...document.querySelectorAll('[role=option]')].find(option => option.textContent === label).click();
            await settle();
            await new Promise(requestAnimationFrame);
          };
          await selectProfile('Image service');
          check(vision.enabled && vision.profile_id === 'images' && vision.model === '', 'Selecting a profile keeps the master enabled while the model is empty');
          await selectModel('image-model');
          check(vision.model === 'image-model', 'Model can be configured after enabling vision');
          await selectModel('settings.vision.selectModel');
          check(vision.enabled && vision.model === '', 'Clearing the model does not disable vision or hide its settings');
          await selectModel('image-model');
          await selectProfile('settings.vision.noProfile');
          check(vision.enabled && vision.profile_id === null && vision.model === '', 'Clearing the profile keeps the settings editable');
          await selectProfile('Image service');
          await selectModel('image-model');
          visionMaster.click();
          await wait(() => !visionCard.querySelector('.vr-overlay-field-grid'));
          collapsed(visionCard, 'Vision');
          check(visionCard.querySelectorAll('button').length === 1, 'Disabled vision has no edit action');
          const snapshots = { ocr: JSON.stringify(ocr), vision: JSON.stringify(vision) };
          for (const item of [
            { name: 'OCR', card: ocrCard, master: ocrMaster, get: () => ocr, key: 'master_enabled', snapshot: snapshots.ocr,
              desktop: 'ocr-desktop-heading', vr: 'ocr-vr-heading' },
            { name: 'Vision', card: visionCard, master: visionMaster, get: () => vision, key: 'enabled', snapshot: snapshots.vision,
              desktop: 'vision-desktop-heading', vr: 'vision-wrist-heading' },
          ]) {
            item.master.click();
            await wait(() => item.card.querySelector('section'));
            checkMasterHeading(item.card, item.master, item.name);
            const desktop = item.card.querySelector('section[aria-labelledby=' + item.desktop + ']');
            const vr = item.card.querySelector('section[aria-labelledby=' + item.vr + ']');
            collapsed(desktop, item.name + ' desktop'); collapsed(vr, item.name + ' VR');
            checkMasterHeading(desktop, desktop.querySelector('[role=switch]'), item.name + ' desktop');
            checkMasterHeading(vr, vr.querySelector('[role=switch]'), item.name + ' VR');
            for (const heading of [item.desktop, item.vr]) {
              const group = item.card.querySelector('section[aria-labelledby=' + heading + ']');
              const toggle = group.querySelector('[role=switch]');
              await wait(() => !toggle.disabled);
              toggle.click();
              await wait(() => group.querySelector('button:not([role=switch]), input, .dropdown-trigger'));
              checkMasterHeading(group, toggle, item.name + ' mode');
              check(item.get().desktop_enabled === (heading === item.desktop), item.name + ': desktop state is independent');
              check(item.get()[item.name === 'OCR' ? 'enabled' : 'vr_enabled'] === (heading === item.vr), item.name + ': VR state is independent');
              item.master.click();
              await wait(() => !item.card.querySelector('section'));
              collapsed(item.card, item.name);
              check(item.get().desktop_enabled === (heading === item.desktop), item.name + ': master preserves desktop state');
              check(item.get()[item.name === 'OCR' ? 'enabled' : 'vr_enabled'] === (heading === item.vr), item.name + ': master preserves VR state');
              item.master.click();
              await wait(() => item.card.querySelector('section[aria-labelledby=' + group.getAttribute('aria-labelledby') + ']'));
              const restored = item.card.querySelector('section[aria-labelledby=' + group.getAttribute('aria-labelledby') + ']');
              check(restored.querySelector('button:not([role=switch]), input, .dropdown-trigger'), item.name + ': enabled mode settings are restored');
              restored.querySelector('[role=switch]').click();
              await wait(() => restored.querySelectorAll('button').length === 1);
              collapsed(restored, item.name + ' mode');
              checkMasterHeading(restored, restored.querySelector('[role=switch]'), item.name + ' mode');
            }
            item.master.click();
            await wait(() => !item.card.querySelector('section'));
            collapsed(item.card, item.name);
            check(JSON.stringify(item.get()) === item.snapshot, item.name + ': all configuration is retained');
          }
          ocrMaster.click(); visionMaster.click();
          await wait(() => ocr.master_enabled && vision.enabled);
          window.setVrAvailable(false);
          await wait(() => !ocrCard.querySelector('[aria-labelledby=ocr-vr-heading]'));
          check(visionCard.querySelector('[aria-labelledby=vision-wrist-heading] [role=switch]').disabled, 'Vision VR stays unavailable without VR support');
          root.unmount();
          document.getElementById('result').textContent = 'PASS';
        } catch (error) { document.getElementById('result').textContent = 'FAIL: ' + error.message; }
      ` },
      bundle: true, write: false, outfile: "vision-settings-test.js", format: "esm", platform: "browser", jsx: "automatic",
      define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
    });
    const directory = mkdtempSync(join(testDirectory, ".vision-settings-browser-"));
    try {
      const html = join(directory, "test.html");
      const javascript = bundle.outputFiles.find(file => file.path.endsWith('.js')).text;
      const stylesheet = bundle.outputFiles.find(file => file.path.endsWith('.css')).text;
      writeFileSync(html, '<style>' + stylesheet + '</style><div id="root"></div><pre id="result">PENDING</pre><script type="module">'
        + javascript.replace(/<\/script/gi, "<\\/script") + '</script>');
      const result = spawnSync(browser, [
        "--headless=new", "--dump-dom", "--virtual-time-budget=15000", "--disable-gpu", "--no-first-run",
        "--disable-background-networking", "--disable-extensions", backend === "local" ? "--window-size=500,800" : "--window-size=1200,900", "--user-data-dir=" + join(directory, "profile"),
        pathToFileURL(html).href,
      ], { encoding: "utf8", timeout: 30000, maxBuffer: 8 * 1024 * 1024 });
      assert.ifError(result.error);
      assert.equal(result.status, 0, result.stderr);
      const report = result.stdout.match(/<pre id="result">([^<]*)<\/pre>/)?.[1];
      assert.equal(report, "PASS", report || result.stderr);
    } finally {
      rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
    }
  });
}
