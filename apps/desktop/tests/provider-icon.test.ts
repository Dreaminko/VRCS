import assert from "node:assert/strict";
import test from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

import AlibabaCloud from "@lobehub/icons/es/AlibabaCloud/components/Color";
import DeepL from "@lobehub/icons/es/DeepL/components/Color";
import DeepSeek from "@lobehub/icons/es/DeepSeek/components/Color";
import Gemini from "@lobehub/icons/es/Gemini/components/Color";
import Groq from "@lobehub/icons/es/Groq/components/Mono";
import LmStudio from "@lobehub/icons/es/LmStudio/components/Mono";
import Microsoft from "@lobehub/icons/es/Microsoft/components/Color";
import Ollama from "@lobehub/icons/es/Ollama/components/Mono";
import OpenAI from "@lobehub/icons/es/OpenAI/components/Mono";
import OpenRouter from "@lobehub/icons/es/OpenRouter/components/Color";
import Qwen from "@lobehub/icons/es/Qwen/components/Color";
import XiaomiMiMo from "@lobehub/icons/es/XiaomiMiMo/components/Mono";

import { providerIcon } from "../src/providers/provider-icon.ts";

const providerIcons = [
  ["openai", OpenAI, "mono"],
  ["gemini", Gemini, "color"],
  ["groq", Groq, "mono"],
  ["deepseek", DeepSeek, "color"],
  ["mimo", XiaomiMiMo, "mono"],
  ["openrouter", OpenRouter, "color"],
  ["alibaba_cloud", AlibabaCloud, "color"],
  ["alibaba_token_plan", AlibabaCloud, "color"],
  ["qwen_ai", Qwen, "color"],
  ["qwen_local", Qwen, "color"],
  ["lm_studio", LmStudio, "mono"],
  ["ollama", Ollama, "mono"],
  ["deepl", DeepL, "color"],
  ["microsoft_translator", Microsoft, "color"],
] as const;

test("catalog providers resolve to their lobe icon mark", () => {
  for (const [provider, icon] of providerIcons) {
    assert.equal(providerIcon({ provider }), icon, provider);
  }
});

test("providers without a brand mark use the initial fallback", () => {
  for (const provider of ["openai_compatible", "custom", ""]) {
    assert.equal(providerIcon({ provider }), null, provider);
  }
});

test("brand marks render the official palette", () => {
  for (const [provider, icon, variant] of providerIcons) {
    if (variant !== "color") continue;
    const markup = renderToStaticMarkup(createElement(icon, { "aria-hidden": true, size: 18 }));
    assert.match(markup, /#[0-9A-Fa-f]{6}/, provider);
    assert.match(markup, /height="18"/, provider);
    assert.doesNotMatch(markup, /fill="currentColor"/, provider);
  }
});

test("brands without a color variant inherit the theme color", () => {
  for (const [provider, icon, variant] of providerIcons) {
    if (variant !== "mono") continue;
    const markup = renderToStaticMarkup(createElement(icon, { "aria-hidden": true, size: 18 }));
    assert.match(markup, /fill="currentColor"/, provider);
    assert.doesNotMatch(markup, /#[0-9A-Fa-f]{6}/, provider);
  }
});
