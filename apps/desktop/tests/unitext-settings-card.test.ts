import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { build } from "esbuild";
import * as React from "react";
import * as ReactDom from "react-dom";
import { renderToStaticMarkup } from "react-dom/server";
import * as ReactJsxRuntime from "react/jsx-runtime";

import type { UniTextSettings } from "../src/integrations/types.ts";

const UNITEXT_REPOSITORY_URL = "https://github.com/Dreaminko/UniText";

type AnchorProps = {
  href?: string;
  target?: string;
  rel?: string;
  onClick?: (event: { preventDefault: () => void }) => void;
};

type SwitchProps = {
  role?: string;
  "aria-label"?: string;
  "aria-checked"?: boolean;
  onClick?: () => void;
};

type JsonObject = Record<string, unknown>;

const enTranslation = (JSON.parse(
  readFileSync(new URL("../src/i18n/locales/en-US.json", import.meta.url), "utf8"),
) as { translation: JsonObject }).translation;

function translate(key: string): string {
  let current: unknown = enTranslation;
  for (const segment of key.split(".")) {
    if (!current || typeof current !== "object" || !(segment in current)) return key;
    current = (current as JsonObject)[segment];
  }
  return typeof current === "string" ? current : key;
}

const defaultConfig: UniTextSettings = {
  enabled: false,
  tier: "auto",
  port: 9000,
  interval_ms: 140,
  clear_after_seconds: 15,
  include_microphone: true,
  include_speaker: true,
  preserve_original_text: true,
  show_player_id: true,
  mute_sync_enabled: true,
};

async function loadUniTextSettingsCard(t: test.TestContext) {
  const key = `unitextCardTest${Math.random().toString(36).slice(2)}`;
  const capturedLinks: AnchorProps[] = [];
  const capturedSwitches: SwitchProps[] = [];
  const invokeCalls: Array<[string, unknown]> = [];
  let tauriRuntime = false;
  let invokeImpl: (command: string, args?: unknown) => Promise<unknown> = async () => undefined;

  const reactBridge = { ...React, useLayoutEffect: React.useEffect };
  const captureElement = (type: unknown, props: Record<string, unknown>) => {
    if (type === "a") capturedLinks.push(props as AnchorProps);
    if (type === "button" && props?.role === "switch") capturedSwitches.push(props as SwitchProps);
  };
  const jsxBridge = {
    Fragment: ReactJsxRuntime.Fragment,
    jsx(type: unknown, props: Record<string, unknown>, itemKey?: unknown) {
      captureElement(type, props);
      return ReactJsxRuntime.jsx(type as never, props, itemKey as never);
    },
    jsxs(type: unknown, props: Record<string, unknown>, itemKey?: unknown) {
      captureElement(type, props);
      return ReactJsxRuntime.jsxs(type as never, props, itemKey as never);
    },
  };

  Object.assign(globalThis, {
    [key]: {
      react: reactBridge,
      reactDom: ReactDom,
      jsxRuntime: jsxBridge,
      t: translate,
      isTauri: () => tauriRuntime,
      invoke: (command: string, args?: unknown) => {
        invokeCalls.push([command, args]);
        return invokeImpl(command, args);
      },
    },
  });
  t.after(() => {
    Reflect.deleteProperty(globalThis, key);
  });

  const bundle = await build({
    entryPoints: [fileURLToPath(new URL("../src/settings/sections/UniTextSettingsCard.tsx", import.meta.url))],
    bundle: true,
    write: false,
    format: "esm",
    platform: "node",
    jsx: "automatic",
    define: { "import.meta.env.VITE_VRCS_SESSION_TOKEN": '""' },
    plugins: [{
      name: "unitext-card-test-boundaries",
      setup(builder) {
        builder.onResolve({ filter: /^(react|react\/jsx-runtime|react-dom|react-i18next|@tauri-apps\/api\/core)$/ }, (args) => ({
          path: args.path,
          namespace: "test-boundary",
        }));
        builder.onLoad({ filter: /.*/, namespace: "test-boundary" }, (args) => {
          if (args.path === "react") {
            return {
              contents: `
                const r = globalThis.${key}.react;
                export const {
                  Fragment, StrictMode, createContext, createElement, forwardRef, memo,
                  useCallback, useContext, useEffect, useId, useImperativeHandle,
                  useLayoutEffect, useMemo, useReducer, useRef, useState,
                } = r;
                export default r;
              `,
            };
          }
          if (args.path === "react/jsx-runtime") {
            return { contents: `export const { Fragment, jsx, jsxs } = globalThis.${key}.jsxRuntime;` };
          }
          if (args.path === "react-dom") {
            return { contents: `export const { createPortal, flushSync } = globalThis.${key}.reactDom;` };
          }
          if (args.path === "react-i18next") {
            return { contents: `export const useTranslation = () => ({ t: globalThis.${key}.t });` };
          }
          return {
            contents: `
              export const invoke = (...args) => globalThis.${key}.invoke(...args);
              export const isTauri = () => globalThis.${key}.isTauri();
            `,
          };
        });
      },
    }],
  });

  const mod = await import(
    `data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString("base64")}`
  ) as typeof import("../src/settings/sections/UniTextSettingsCard.tsx");

  return {
    UniTextSettingsCard: mod.UniTextSettingsCard,
    capturedLinks,
    capturedSwitches,
    invokeCalls,
    setTauriRuntime(value: boolean) { tauriRuntime = value; },
    setInvokeImpl(impl: (command: string, args?: unknown) => Promise<unknown>) { invokeImpl = impl; },
  };
}

test("UniText heading keeps plain-text title and renders adjacent repository icon link when collapsed and expanded", async (t) => {
  const { UniTextSettingsCard, capturedLinks } = await loadUniTextSettingsCard(t);
  const expectedLabel = translate("settings.unitext.openRepository");
  assert.equal(expectedLabel, "Open UniText repository");

  for (const enabled of [false, true]) {
    capturedLinks.length = 0;
    const html = renderToStaticMarkup(
      React.createElement(UniTextSettingsCard, {
        config: { ...defaultConfig, enabled },
        runtime: undefined,
        saveState: "idle",
        onChange: () => {},
      }),
    );

    const match = html.match(
      /<div class="connections-heading-title"><h3 id="connections-unitext-title">([^<]*)<\/h3>(<a [^>]*>)([\s\S]*?)<\/a><\/div>/,
    );
    assert.ok(match, `Expected heading title wrapper and adjacent anchor when enabled=${String(enabled)}`);
    assert.equal(match[1], "UniText");
    assert.ok(!match[1].includes("<a"), "Title heading must remain plain text without nested link");

    const anchorTag = match[2];
    assert.ok(anchorTag.includes('class="connections-title-link-button"'));
    assert.ok(anchorTag.includes(`href="${UNITEXT_REPOSITORY_URL}"`));
    assert.ok(anchorTag.includes('target="_blank"'));
    assert.ok(anchorTag.includes('rel="noopener noreferrer"'));
    assert.ok(anchorTag.includes(`aria-label="${expectedLabel}"`));
    assert.ok(anchorTag.includes(`data-tooltip="${expectedLabel}"`));
    assert.ok(!/\stitle=/.test(anchorTag), "Repository link must use data-tooltip instead of native title");
    assert.ok(match[3].includes("<svg"), "Repository link should render ExternalLink icon");
    assert.equal(capturedLinks.length, 1);
  }
});

test("UniText repository link preserves browser navigation outside Tauri and invokes open_project_link in Tauri", async (t) => {
  const { UniTextSettingsCard, capturedLinks, invokeCalls, setTauriRuntime, setInvokeImpl } =
    await loadUniTextSettingsCard(t);

  renderToStaticMarkup(
    React.createElement(UniTextSettingsCard, {
      config: defaultConfig,
      runtime: undefined,
      saveState: "idle",
      onChange: () => {},
    }),
  );

  assert.equal(capturedLinks.length, 1);
  const onClick = capturedLinks[0].onClick;
  assert.ok(onClick, "Expected anchor onClick handler");
  assert.equal(typeof onClick, "function");

  setTauriRuntime(false);
  let preventedOutsideTauri = false;
  onClick({ preventDefault: () => { preventedOutsideTauri = true; } });
  assert.equal(preventedOutsideTauri, false);
  assert.deepEqual(invokeCalls, []);

  setTauriRuntime(true);
  let preventedInTauri = false;
  onClick({ preventDefault: () => { preventedInTauri = true; } });
  assert.equal(preventedInTauri, true);
  assert.deepEqual(invokeCalls, [
    ["open_project_link", { target: UNITEXT_REPOSITORY_URL }],
  ]);

  const errors: unknown[][] = [];
  const originalConsoleError = console.error;
  console.error = (...args: unknown[]) => { errors.push(args); };
  t.after(() => { console.error = originalConsoleError; });

  setInvokeImpl(() => Promise.reject(new Error("open failed")));
  onClick({ preventDefault: () => {} });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(errors.length, 1);
});

test("UniText show_player_id toggle only renders when per-player audio tracks are enabled", async (t) => {
  const { UniTextSettingsCard, capturedSwitches } = await loadUniTextSettingsCard(t);
  const label = translate("settings.unitext.showPlayerId");
  const description = translate("settings.unitext.showPlayerIdDescription");

  for (const playerAudioEnabled of [undefined, false]) {
    capturedSwitches.length = 0;
    const html = renderToStaticMarkup(
      React.createElement(UniTextSettingsCard, {
        config: { ...defaultConfig, enabled: true },
        runtime: undefined,
        saveState: "idle",
        playerAudioEnabled,
        onChange: () => {},
      }),
    );
    assert.ok(!html.includes(label), `showPlayerId should be hidden when playerAudioEnabled=${String(playerAudioEnabled)}`);
    assert.ok(!capturedSwitches.some((item) => item["aria-label"] === label));
  }

  const patches: Array<Partial<UniTextSettings>> = [];
  for (const show_player_id of [true, false]) {
    capturedSwitches.length = 0;
    const html = renderToStaticMarkup(
      React.createElement(UniTextSettingsCard, {
        config: { ...defaultConfig, enabled: true, show_player_id },
        runtime: undefined,
        saveState: "idle",
        playerAudioEnabled: true,
        onChange: (patch) => patches.push(patch),
      }),
    );
    const normalizedHtml = html.replaceAll("&#x27;", "'");
    assert.ok(normalizedHtml.includes(label));
    assert.ok(normalizedHtml.includes(description));
    const toggle = capturedSwitches.find((item) => item["aria-label"] === label);
    assert.ok(toggle, "Expected showPlayerId switch");
    assert.equal(toggle["aria-checked"], show_player_id);
    toggle.onClick?.();
  }

  assert.deepEqual(patches, [
    { show_player_id: false },
    { show_player_id: true },
  ]);
});
