import assert from "node:assert/strict";
import test from "node:test";
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
  ["openai", OpenAI],
  ["gemini", Gemini],
  ["groq", Groq],
  ["deepseek", DeepSeek],
  ["mimo", XiaomiMiMo],
  ["openrouter", OpenRouter],
  ["alibaba_cloud", AlibabaCloud],
  ["alibaba_token_plan", AlibabaCloud],
  ["qwen_ai", Qwen],
  ["qwen_local", Qwen],
  ["lm_studio", LmStudio],
  ["ollama", Ollama],
  ["deepl", DeepL],
  ["microsoft_translator", Microsoft],
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
