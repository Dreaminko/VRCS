import assert from "node:assert/strict";
import test from "node:test";

import { thinkingControlForModel } from "../src/translation-thinking.ts";

test("Groq exposes controls according to each model API", () => {
  assert.equal(thinkingControlForModel("groq", "qwen/qwen3.6-27b"), "disable_supported");
  assert.equal(thinkingControlForModel("groq", "openai/gpt-oss-120b"), "hide_only");
  assert.equal(thinkingControlForModel("groq", "minimaxai/minimax-m2.7"), "hide_only");
  assert.equal(thinkingControlForModel("groq", "llama-3.3-70b-versatile"), "unsupported");
});

test("DeepSeek and Alibaba distinguish toggleable and thinking-only models", () => {
  assert.equal(thinkingControlForModel("deepseek", "deepseek-v4-flash"), "disable_supported");
  assert.equal(thinkingControlForModel("alibaba_cloud", "qwen-plus"), "disable_supported");
  assert.equal(thinkingControlForModel("alibaba_cloud", "deepseek-r1"), "hide_only");
  assert.equal(thinkingControlForModel("alibaba_token_plan", "qwen3.8-max"), "disable_supported");
});

test("custom OpenAI-compatible profiles do not inherit branded parameters", () => {
  assert.equal(
    thinkingControlForModel("openai_compatible", "qwen/qwen3.6-27b"),
    "unsupported",
  );
});
