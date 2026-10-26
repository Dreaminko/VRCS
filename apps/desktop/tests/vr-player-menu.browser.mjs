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
  const locales = Object.fromEntries(["en-US", "zh-CN", "ja-JP", "zh-Hant"].map(locale => [locale,
    JSON.parse(readFileSync(join(resolveDir, "src/i18n/locales", locale + ".json"), "utf8")).translation.live.players]));
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
        detail: i === 12 ? 'Saved selection' : i < 7 ? 'Current world' : 'Historical track',
        current: i < 7, selected: i === 0 || i === 12, hidden: i === 1,
      }));
      let revision = 0;
      window.renderMenu = (options = {}) => {
        const controls = { owner_epoch: 1, revision: 1, dark: !!options.dark, labels: {},
          items: ['Wrist', 'Headset', 'OSC', 'Capture'].map((label, i) => ({ label, requested: i !== 3, effective: i !== 3, available: true, busy: false, reason: null })) };
        window.menuState = { key: { generation: 1, revision: ++revision }, hovered: options.hovered ?? null, pressed: options.pressed ?? null, pointer: null,
          view: { kind: 'menu', controls, hold_progress: options.hold_progress ?? null,
            ui: { page: options.page ?? 'players', group: options.group ?? 'current', player_page: options.player_page ?? 0, language_page: options.language_page ?? 0 },
            players: { available: true, processing_available: true, player_mode: true, selection_mode: 'selected', selected_count: 2,
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
      assert.deepEqual(await page.locator(".vr-menu-capsule [data-vr-region]").evaluateAll(nodes => nodes.map(node => +node.dataset.vrRegion)), [0, 1, 2, 4, 3]);
      const quick = await page.locator(".vr-menu-capsule [data-vr-region]").evaluateAll(nodes => nodes.map(node => ({ id: +node.dataset.vrRegion, width: node.offsetWidth, height: node.offsetHeight, command: JSON.parse(node.dataset.vrMenu) })));
      assert.deepEqual(quick.map(item => [item.width, item.height]), Array(5).fill([160, 160]));
      assert.deepEqual(quick.map(item => item.command), [
        { control: { set_wrist_enabled: false } }, { control: { set_headset_enabled: false } }, { control: { set_osc_enabled: false } },
        { page: "players" }, { control: { set_capture_running: true } },
      ]);
      await page.screenshot({ path: join(screenshots, `menu-home-${dark ? "dark" : "light"}.png`) });
      for (const panelPage of ["players", "languages"]) {
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

test("player and language pages paginate identities, gate writes, and fit all supported locales", { timeout: 60000 }, async () => {
  const { browser, page, locales } = await setup();
  try {
    assert.equal(await page.locator(".vr-menu-player-row").count(), 5);
    assert.equal(await commandButton(page, { display: { set_processing_mode: "selected" } }).isDisabled(), true, "Re-selecting the active mode must not clear the saved selection");
    assert.equal(await commandButton(page, { display: { set_user_selected: { user_id: "usr_0", selected: false } } }).count(), 1);
    assert.equal(await commandButton(page, { display: { set_user_selected: { user_id: "usr_1", selected: true } } }).count(), 1, "Same display names retain distinct user IDs");
    assert.equal(await commandButton(page, { display: { set_track_hidden: { track_id: "track-1", hidden: false } } }).count(), 1);
    assert.equal(await page.locator('.vr-menu-player-row').nth(2).locator('[data-vr-menu]').first().isDisabled(), true, "Unknown identity cannot select processing");
    assert.equal(await commandButton(page, { player_page: 1 }).isDisabled(), false);
    await render(page, { player_page: 99 });
    assert.equal(await page.locator(".vr-menu-player-row").count(), 2, "Shrinking or stale pages clamp to the last populated page");
    assert.equal(await commandButton(page, { player_page: 0 }).isDisabled(), false);
    await render(page, { group: "history", player_page: 1 });
    assert.equal(await page.locator(".vr-menu-player-row").count(), 1);
    assert.equal(await commandButton(page, { display: { set_user_selected: { user_id: "usr_12", selected: false } } }).isDisabled(), false);
    assert.equal(await page.locator('.vr-menu-player-row [data-vr-menu]').last().isDisabled(), true, "Saved selection without a track cannot hide records");

    await render(page, { players: { busy: true, error: "The setting could not be saved. Try again." } });
    assert.ok((await page.locator('[data-vr-menu]').evaluateAll(nodes => nodes.filter(node => JSON.parse(node.dataset.vrMenu).display).every(node => node.disabled))));
    assert.equal(await commandButton(page, { page: "languages" }).isDisabled(), false);
    assert.equal(await page.locator('.vr-menu-capsule [data-vr-region="3"]').isDisabled(), false);
    assert.match(await page.locator('[role="alert"]').innerText(), /could not be saved/);
    await render(page, { players: { available: false, processing_available: false } });
    assert.ok(await page.locator('[data-vr-menu]').evaluateAll(nodes => nodes.filter(node => JSON.parse(node.dataset.vrMenu).display).every(node => node.disabled)));
    assert.equal(await commandButton(page, { page: "home" }).first().isDisabled(), false);
    await render(page, { page: "home", players: { available: false, processing_available: false, player_mode: false } });
    assert.equal(await commandButton(page, { page: "players" }).isDisabled(), false, "The entry remains available without a main owner or player audio");
    await render(page, { players: { processing_available: false, player_mode: false } });
    assert.equal(await commandButton(page, { display: { set_processing_mode: "all" } }).isDisabled(), true);
    assert.equal(await commandButton(page, { display: { set_track_hidden: { track_id: "track-0", hidden: true } } }).isDisabled(), false, "Core loss must not disable local record visibility");
    await render(page, { players: { selection_mode: "all" } });
    assert.equal(await commandButton(page, { display: { set_processing_mode: "all" } }).isDisabled(), true);
    assert.equal(await commandButton(page, { display: { set_user_selected: { user_id: "usr_0", selected: false } } }).isDisabled(), true);
    await render(page, { players: { players: [] } });
    assert.equal(await page.locator(".vr-menu-player-row").count(), 0);
    assert.equal(await commandButton(page, { display: { select_current: null } }).isDisabled(), true);
    assert.equal(await commandButton(page, { display: { show_all_tracks: null } }).isDisabled(), false, "Show all also clears hidden tracks absent from the current snapshot");

    await render(page, { page: "languages" });
    assert.equal(await page.locator(".vr-menu-language-option").count(), 12);
    assert.equal(await commandButton(page, { display: { set_source_language_hidden: { language: "lang-1", hidden: false } } }).count(), 1);
    await render(page, { page: "languages", language_page: 99 });
    assert.equal(await page.locator(".vr-menu-language-option").count(), 2);
    await render(page, { page: "languages", players: { follow_default: true } });
    assert.ok(await page.locator(".vr-menu-language-option").evaluateAll(nodes => nodes.every(node => node.disabled)));
    assert.equal(await commandButton(page, { display: { clear_hidden_languages: null } }).isDisabled(), false);
    assert.equal(await commandButton(page, { display: { set_follow_default: false } }).isDisabled(), false);

    for (const locale of Object.keys(locales)) for (const dark of [false, true]) for (const panelPage of ["players", "languages"]) {
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
