import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const { chromium } = createRequire(import.meta.url)("playwright");
const testDirectory = dirname(fileURLToPath(import.meta.url));
const resolveDir = resolve(testDirectory, "..");
const screenshots = resolve(resolveDir, "../../.build/wrist-menu");
const executablePath = [process.env.PROGRAMFILES, process.env["PROGRAMFILES(X86)"], process.env.LOCALAPPDATA]
  .filter(Boolean).flatMap(root => [join(root, "Google/Chrome/Application/chrome.exe"), join(root, "Microsoft/Edge/Application/msedge.exe")]).find(existsSync);

async function setup() {
  const locales = Object.fromEntries(["en-US", "zh-CN", "ja-JP", "zh-Hant"].map(locale => {
    const t = JSON.parse(readFileSync(join(resolveDir, "src/i18n/locales", locale + ".json"), "utf8")).translation;
    return [locale, {
      ...t.live.players,
      close: t.common.close, previous: t.wristReader.previousPage, next: t.wristReader.nextPage,
    }];
  }));
  const bundle = await build({
    stdin: { resolveDir, loader: "tsx", contents: `
      import React from 'react';
      import { createRoot } from 'react-dom/client';
      import { flushSync } from 'react-dom';
      import { VrSurface } from './src/vr-surface/VrSurface';
      import './src/styles.css';
      const root = createRoot(document.getElementById('root'));
      const locales = ${JSON.stringify(locales)};
      const languages = ['English', 'Japanese', 'Chinese (Simplified)', 'Chinese (Traditional)', 'Korean', 'French', 'German', 'Spanish', 'Portuguese', 'Indonesian', 'Vietnamese', 'Ukrainian', 'Arabic', 'Italian'];
      const players = Array.from({ length: 13 }, (_, i) => ({
        track_id: i === 12 ? null : 'track-' + i,
        user_id: i === 2 ? null : 'usr_' + i,
        name: i < 2 ? 'Shared name' : i === 3 ? 'A very long multilingual player name \\u6d4b\\u8bd5\\u30c6\\u30b9\\u30c8 '.repeat(6) : i === 2 ? 'Identity pending' : 'Player ' + i,
        detail: i === 12 ? 'Saved selection' : i < 7 ? 'Current world · 2.4 m' : 'Historical track',
        current: i < 7, hidden: i === 1, chatbox_disabled: i === 4, voice_disabled: i === 5 || i === 1, voice_locked: i === 5,
      }));
      let revision = 0;
      window.renderMenu = (options = {}) => {
        const controls = { owner_epoch: 1, revision: 1, dark: !!options.dark, labels: {},
          items: ['Wrist', 'Headset', 'OSC', 'Capture', 'Frame gesture'].map((label, i) => ({ label, requested: i !== 3, effective: i !== 3, available: true, busy: false, reason: null,
            ...(i === 4 ? options.gesture : {}) })) };
        window.menuState = { key: { generation: 1, revision: ++revision }, hovered: options.hovered ?? null, pressed: options.pressed ?? null,
          pointer: options.pointer ?? null,
          view: { kind: 'menu', controls, hold_progress: options.hold_progress ?? null,
            ui: { page: options.page ?? 'players', group: options.group ?? 'current', player_page: options.player_page ?? 0, language_page: options.language_page ?? 0 },
            players: { available: true, processing_available: true, player_mode: true,
              source_mode: options.source_mode ?? 'allowlist',
              proximity: { enabled: false, radius_m: 10 },
              view_priority: { enabled: true, horizontal_fov_deg: 110, local_view: true },
              players, languages: languages.map((label, i) => ({ code: 'lang-' + i, label, hidden: i === 1 })),
              follow_default: false, default_languages: ['lang-1'], labels: locales[options.locale ?? 'en-US'], busy: false, error: null,
              ...options.players } } };
        flushSync(() => root.render(<VrSurface state={window.menuState} onLayout={layout => { window.menuLayout = layout; }} />));
      };
      window.renderMenu();
    ` },
    bundle: true, write: false, outfile: "player-menu.js", format: "esm", platform: "browser", jsx: "automatic", minify: true,
    loader: { ".png": "dataurl" },
  });
  const browser = await chromium.launch({ headless: true, ...(executablePath ? { executablePath } : {}) });
  const page = await browser.newPage({ viewport: { width: 1280, height: 1280 }, deviceScaleFactor: 1 });
  await page.setContent('<!doctype html><html><head></head><body><div id="root"></div></body></html>');
  await page.addStyleTag({ content: bundle.outputFiles.find(file => file.path.endsWith(".css")).text + '\nbody { margin: 0; min-width: 0; background: #e7ebf0; }' });
  await page.addScriptTag({ content: bundle.outputFiles.find(file => file.path.endsWith(".js")).text, type: "module" });
  await page.waitForFunction(() => window.menuLayout);
  mkdirSync(screenshots, { recursive: true });
  return { browser, page, locales };
}

const render = (page, options) => page.evaluate(options => window.renderMenu(options), options);
const commandButton = (page, command) => page.locator(`[data-vr-menu='${JSON.stringify(command)}']`);

test("the player panel expands above persistent wrist controls with stable semantic hit regions", { timeout: 60000 }, async () => {
  const { browser, page } = await setup();
  try {
    assert.equal(await page.locator(".vr-player-display-panel").count(), 1, "The settings panel must render above the quick capsule");
    for (const dark of [false, true]) {
      await render(page, { dark, page: "home" });
      const capsule = await page.locator(".vr-menu-capsule").boundingBox();
      assert.deepEqual(capsule, { x: 16, y: 1032, width: 1248, height: 240 });
      assert.deepEqual(await page.locator(".vr-menu-capsule [data-vr-region]").evaluateAll(nodes => nodes.map(node => +node.dataset.vrRegion)), [0, 1, 2, 30, 4, 3]);
      const quick = await page.locator(".vr-menu-capsule [data-vr-region]").evaluateAll(nodes => nodes.map(node => ({ id: +node.dataset.vrRegion, width: node.offsetWidth, height: node.offsetHeight, command: JSON.parse(node.dataset.vrMenu) })));
      assert.deepEqual(quick.map(item => [item.width, item.height]), Array(6).fill([160, 160]));
      assert.deepEqual(quick.map(item => item.command), [
        { control: { set_wrist_enabled: false } }, { control: { set_headset_enabled: false } }, { control: { set_osc_enabled: false } },
        { control: { set_ocr_gesture_enabled: false } },
        { page: "players" }, { control: { set_capture_running: true } },
      ]);
      await page.screenshot({ path: join(screenshots, `menu-home-${dark ? "dark" : "light"}.png`) });
      for (const panelPage of ["players", "rules", "languages"]) {
        await render(page, { dark, page: panelPage });
        assert.deepEqual(await page.locator(".vr-menu-capsule").boundingBox(), capsule, "Opening a page must not move or resize the wrist controls");
        assert.deepEqual(await page.locator(".vr-player-display-panel").boundingBox(), { x: 16, y: 16, width: 1248, height: 992 });
        const layout = await page.evaluate(() => window.menuLayout);
        assert.deepEqual(layout.menu_bounds, [0.0125, 0.80625, 0.9875, 0.99375]);
        assert.equal(layout.panels.length, 1);
        assert.deepEqual(layout.panels[0].bounds, [0.0125, 0.0125, 0.9875, 0.7875]);
        assert.ok(layout.regions.length <= 64);
        assert.equal(new Set(layout.regions.map(region => region.id)).size, layout.regions.length);
        assert.ok(layout.regions.every(region => region.menu && !region.dashboard));
        const panelRegions = await page.locator(".vr-player-display-panel [data-vr-region]").evaluateAll(nodes => nodes.map(node => ({ id: +node.dataset.vrRegion, width: node.offsetWidth, height: node.offsetHeight })));
        assert.ok(panelRegions.every(region => region.id >= 5 && region.id < 64 && region.width >= 72 && region.height >= 72));
        for (let i = 0; i < layout.regions.length; i++) for (let j = i + 1; j < layout.regions.length; j++) {
          const a = layout.regions[i].bounds, b = layout.regions[j].bounds;
          assert.ok(a[2] <= b[0] || b[2] <= a[0] || a[3] <= b[1] || b[3] <= a[1], "Action hit rectangles must not overlap");
        }
        const before = JSON.stringify(layout);
        await commandButton(page, { page: "home" }).first().evaluate(button => button.click());
        assert.equal(await page.evaluate(() => JSON.stringify(window.menuLayout)), before, "DOM clicks do not dispatch native menu commands");
      }
    }
  } finally { await browser.close(); }
});

test("the wrist photo gesture control exposes saved state and gates unavailable or pending writes", { timeout: 60000 }, async () => {
  const { browser, page } = await setup();
  try {
    for (const dark of [false, true]) for (const enabled of [false, true]) {
      await render(page, { dark, page: "home", hovered: 30, gesture: { requested: enabled, effective: enabled } });
      const button = commandButton(page, { control: { set_ocr_gesture_enabled: !enabled } });
      assert.equal(await button.getAttribute("aria-pressed"), String(enabled));
      assert.equal(await button.isDisabled(), false);
      assert.equal(await button.locator("svg.lucide-crop").count(), 1);
      assert.match(await button.getAttribute("aria-label"), /Frame gesture/);
      assert.deepEqual(await page.evaluate(() => window.menuLayout.regions.find(region => region.id === 30)?.menu),
        { control: { set_ocr_gesture_enabled: !enabled } });
      for (const gesture of [{ busy: true }, { available: false }]) {
        await render(page, { dark, gesture: { requested: enabled, effective: enabled, ...gesture } });
        assert.equal(await commandButton(page, { control: { set_ocr_gesture_enabled: !enabled } }).isDisabled(), true);
        assert.equal(await page.evaluate(() => window.menuLayout.regions.some(region => region.id === 30)), false);
      }
      await render(page, { dark, page: "home", gesture: { requested: enabled, effective: enabled, hidden: true } });
      assert.equal(await page.locator('[data-vr-region="30"]').count(), 0);
      assert.equal(await commandButton(page, { control: { set_ocr_gesture_enabled: !enabled } }).count(), 0);
      assert.deepEqual(await page.locator(".vr-menu-capsule").boundingBox(), { x: 16, y: 1032, width: 1248, height: 240 });
      const hiddenCapsule = await page.locator(".vr-menu-capsule [data-vr-region]").evaluateAll(nodes => nodes.map(node => ({ id: +node.dataset.vrRegion, width: node.offsetWidth, height: node.offsetHeight })));
      assert.deepEqual(hiddenCapsule.map(item => item.id), [0, 1, 2, 4, 3]);
      assert.deepEqual(hiddenCapsule.map(item => [item.width, item.height]), Array(5).fill([160, 160]));
      assert.equal(await page.evaluate(() => window.menuLayout.regions.some(region => region.id === 30)), false);

      await render(page, { dark, page: "home", gesture: { requested: enabled, effective: enabled, hidden: false } });
      assert.equal(await page.locator('[data-vr-region="30"]').count(), 1);
      assert.equal(await page.evaluate(() => window.menuLayout.regions.some(region => region.id === 30)), true);
    }
  } finally { await browser.close(); }
});

test("spatial rules live on the Rules tab with full-width sliders and quick-off action", { timeout: 60000 }, async () => {
  const { browser, page } = await setup();
  try {
    await render(page, { page: "players" });
    assert.equal(await page.locator(".vr-menu-proximity-track").count(), 0, "Sliders move off the People page");
    assert.equal(await commandButton(page, { page: "rules" }).count(), 2, "People exposes Rules tab and footer summary shortcut");

    await render(page, { page: "rules" });
    const slider = page.locator('[data-vr-region="32"]');
    const fovSlider = page.locator('[data-vr-region="51"]');
    assert.equal(await slider.count(), 1);
    assert.equal(await fovSlider.count(), 1);
    assert.equal(await slider.getAttribute("role"), "slider");
    assert.equal(await slider.getAttribute("aria-valuemin"), "0");
    assert.equal(await slider.getAttribute("aria-valuemax"), "10");
    assert.equal(await slider.isDisabled(), false, "The slider stays usable while filtering is off");
    assert.equal(await slider.getAttribute("aria-valuenow"), "0", "Filtering off parks the slider at zero");
    assert.equal(await slider.getAttribute("aria-valuetext"), "Off");
    assert.equal(await page.locator('[data-vr-region="54"]').isDisabled(), true, "Quick-off button disables while already off");
    assert.deepEqual(JSON.parse(await slider.getAttribute("data-vr-menu")),
      { slider: { action: "proximity_radius", min: 0, max: 10, step: 0.1 } });
    assert.deepEqual(JSON.parse(await fovSlider.getAttribute("data-vr-menu")),
      { slider: { action: "view_horizontal_fov", min: 30, max: 180, step: 5 } });

    const track = await slider.boundingBox();
    assert.ok(track && track.height >= 72 && track.width >= 900, "Full-width proximity slider stays easy to drag");

    await render(page, { page: "rules", players: { proximity: { enabled: true, radius_m: 5 }, view_priority: { enabled: true, horizontal_fov_deg: 110, local_view: false } } });
    assert.equal(await page.locator('[data-vr-region="54"]').isDisabled(), false);
    assert.equal(await slider.getAttribute("aria-valuenow"), "5");
    assert.equal(await page.locator(".vr-menu-proximity-value").first().innerText(), "5.0 m");
    assert.match(await page.locator(".vr-menu-card-note").innerText(), /Waiting for view data/);

    const surface = await page.locator(".vr-surface").boundingBox();
    const dragTrack = await slider.boundingBox();
    assert.ok(surface && dragTrack);
    const dragPointer = [
      (dragTrack.x + dragTrack.width * 0.7 - surface.x) / surface.width,
      (dragTrack.y + dragTrack.height / 2 - surface.y) / surface.height,
    ];
    await render(page, { page: "rules", pressed: 32, pointer: dragPointer, players: { proximity: { enabled: true, radius_m: 5 } } });
    assert.equal(await slider.getAttribute("aria-valuenow"), "7");
    assert.equal(await page.locator(".vr-menu-proximity-value").first().innerText(), "7.0 m");
    await render(page, { page: "rules", players: { proximity: { enabled: true, radius_m: 5 } } });
    assert.equal(await page.locator(".vr-menu-proximity-value").first().innerText(), "7.0 m", "Release keeps the dragged value until it is confirmed");

    const failed = "Distance filter was not applied";
    await render(page, { page: "rules", players: { proximity: { enabled: true, radius_m: 5 }, error: failed } });
    await render(page, { page: "rules", pressed: 32, pointer: dragPointer, players: { proximity: { enabled: true, radius_m: 5 }, error: failed } });
    await render(page, { page: "rules", players: { proximity: { enabled: true, radius_m: 5 }, error: null } });
    await render(page, { page: "rules", players: { proximity: { enabled: true, radius_m: 5 }, error: failed } });
    assert.equal(await page.locator(".vr-menu-proximity-value").first().innerText(), "5.0 m", "A repeated failure restores the confirmed value");

    await render(page, { page: "rules", players: { player_mode: false } });
    assert.equal(await slider.isDisabled(), true);
    assert.equal(await fovSlider.isDisabled(), true);
    assert.match(await page.locator('.vr-player-display-panel [role="status"]').innerText(), /per-player audio/);

    await render(page, { page: "rules", players: { proximity: undefined, view_priority: undefined } });
    assert.equal(await slider.count(), 0);
    assert.equal(await fovSlider.count(), 0);
  } finally { await browser.close(); }
});

test("player and language pages paginate cleanly, show active semantics, and fit all locales", { timeout: 60000 }, async () => {
  const { browser, page, locales } = await setup();
  try {
    assert.equal(await page.locator(".vr-menu-player-row").count(), 5);
    assert.equal(await commandButton(page, { display: { set_user_source_disabled: { user_id: "usr_0", source: "vrchat_chatbox", disabled: true } } }).count(), 1);
    assert.equal(await commandButton(page, { display: { set_user_source_disabled: { user_id: "usr_1", source: "vrchat_chatbox", disabled: true } } }).count(), 1, "Same display names retain distinct user IDs");
    const modeRow = await page.locator(".vr-menu-source-segments").boundingBox();
    assert.ok(modeRow && modeRow.height >= 72, "The source mode switch keeps a VR-sized hit target");
    assert.equal(await page.locator(".vr-menu-source-segments").getAttribute("aria-label"), "Player sources");
    await render(page, { source_mode: "everyone" });
    assert.equal(await commandButton(page, { display: { set_source_mode: "everyone" } }).getAttribute("aria-pressed"), "true");
    assert.equal(await commandButton(page, { display: { set_source_mode: "allowlist" } }).getAttribute("aria-pressed"), "false");
    await render(page, { source_mode: "allowlist" });
    assert.equal(await commandButton(page, { display: { set_source_mode: "allowlist" } }).getAttribute("aria-pressed"), "true");
    assert.equal(await page.locator('.vr-menu-player-row').first().locator('[data-vr-menu]').count(), 2, "Player rows offer only Chatbox and voice controls");

    const enabledChatbox = commandButton(page, { display: { set_user_source_disabled: { user_id: "usr_0", source: "vrchat_chatbox", disabled: true } } });
    assert.equal(await enabledChatbox.getAttribute("aria-pressed"), "false");
    assert.equal(await enabledChatbox.evaluate(el => el.classList.contains("active") && !el.classList.contains("is-muted")), true, "Enabled sources highlight cleanly");
    const disabledChatbox = commandButton(page, { display: { set_user_source_disabled: { user_id: "usr_4", source: "vrchat_chatbox", disabled: false } } });
    assert.equal(await disabledChatbox.getAttribute("aria-pressed"), "true");
    assert.equal(await disabledChatbox.evaluate(el => !el.classList.contains("active") && el.classList.contains("is-muted")), true, "Disabled sources render muted");

    assert.equal(await page.locator('.vr-menu-player-row').nth(2).locator('[data-vr-menu]').first().isDisabled(), true, "Unknown identity cannot change source settings");
    await render(page, { player_page: 1 });
    assert.equal(await commandButton(page, { display: { set_user_source_disabled: { user_id: "usr_5", source: "speaker", disabled: false } } }).isDisabled(), true, "Hidden accounts cannot resume voice");
    assert.match(await page.locator('.vr-menu-player-row').first().locator("small").innerText(), /voice locked/);
    await render(page, { player_page: 99 });
    assert.equal(await page.locator(".vr-menu-player-row").count(), 2, "Shrinking or stale pages clamp to the last populated page");
    await render(page, { group: "history", player_page: 1 });
    assert.equal(await page.locator(".vr-menu-player-row").count(), 1);
    assert.equal(await page.locator('.vr-menu-player-row [data-vr-menu]').last().isDisabled(), false, "Saved accounts retain voice controls without a live track");

    await render(page, { players: { busy: true, error: "The setting could not be saved. Try again." } });
    assert.match(await page.locator('[role="alert"]').innerText(), /could not be saved/);
    await render(page, { players: { available: false, processing_available: false } });
    assert.ok(await page.locator('[data-vr-menu]').evaluateAll(nodes => nodes.filter(node => JSON.parse(node.dataset.vrMenu).display).every(node => node.disabled)));
    assert.equal(await commandButton(page, { page: "home" }).first().isDisabled(), false);
    await render(page, { players: { players: [] } });
    assert.equal(await page.locator(".vr-menu-player-row").count(), 0);
    assert.match(await page.locator(".vr-surface-empty").innerText(), /current world/);

    await render(page, { page: "languages" });
    assert.equal(await page.locator(".vr-menu-language-option").count(), 12);
    assert.equal(await commandButton(page, { display: { clear_hidden_languages: null } }).isDisabled(), false);
    await render(page, { page: "languages", language_page: 99 });
    assert.equal(await page.locator(".vr-menu-language-option").count(), 2);
    await render(page, { page: "languages", players: { languages: [{ code: "en", label: "English", hidden: false }] } });
    assert.equal(await commandButton(page, { display: { clear_hidden_languages: null } }).isDisabled(), true);

    for (const locale of Object.keys(locales)) for (const dark of [false, true]) for (const panelPage of ["players", "rules", "languages"]) {
      await render(page, { locale, dark, page: panelPage, hovered: 6, pressed: 7 });
      assert.equal(await page.locator(".vr-player-display-panel h1").innerText(), locales[locale].title);
      const overflow = await page.locator(".vr-player-display-panel").evaluate(panel => {
        const panelRect = panel.getBoundingClientRect();
        return [...panel.querySelectorAll('button, .vr-menu-player-row, .vr-menu-player-name')].filter(node => {
          const rect = node.getBoundingClientRect();
          return rect.left < panelRect.left || rect.right > panelRect.right || rect.top < panelRect.top || rect.bottom > panelRect.bottom;
        }).map(node => node.className);
      });
      assert.deepEqual(overflow, [], `Localized controls must fit: ${locale} ${panelPage}`);
      assert.equal(await page.locator('[data-vr-region="6"]').evaluate(node => node.classList.contains('hovered')), true);
      assert.equal(await page.locator('[data-vr-region="7"]').evaluate(node => node.classList.contains('pressed')), true);
      if (panelPage === "players") assert.ok(await page.locator(".vr-menu-player-row").evaluateAll(rows => rows.every(row => {
        const name = row.querySelector('.vr-menu-player-name').getBoundingClientRect();
        return name.right <= row.querySelector('button').getBoundingClientRect().left;
      })), "Long names must not invade the action columns");
      await page.screenshot({ path: join(screenshots, `menu-${panelPage}-${locale}-${dark ? "dark" : "light"}.png`) });
    }
  } finally { await browser.close(); }
});
