import assert from "node:assert/strict";
import test from "node:test";

import { normalizeReasoningControlForModel, reasoningOptionsForModel, thinkingControlForModel } from "../src/translation/translation-thinking.ts";

test("Gemini budget models expose their actual ability to disable thinking", () => {
  assert.equal(thinkingControlForModel("gemini", "gemini-2.5-flash"), "disable_supported");
  assert.equal(thinkingControlForModel("gemini", "gemini-2.5-pro"), "hide_only");
});

test("reasoning selectors preserve supported values and clear incompatible model selections", () => {
  const details = { reasoning: { supported: true, can_disable: false, supported_efforts: ["high", "low"] } };
  const high = { type: "effort" as const, value: "high" };
  assert.equal(normalizeReasoningControlForModel("openai", "model", high, details), high);
  assert.equal(normalizeReasoningControlForModel("openai", "model", { ...high, value: "medium" }, details), null);
  assert.equal(thinkingControlForModel("openai", "model", details), "hide_only");
  assert.deepEqual(reasoningOptionsForModel("openai_compatible", "model", details).efforts, []);
});

test("budget ranges distinguish dynamic thinking from the minimum positive budget", () => {
  const options = reasoningOptionsForModel("gemini", "gemini-2.5-flash-lite");
  assert.deepEqual(options.budget, { min: 512, max: 24576, dynamic: true });
  for (const value of [-2, 0, 1, 511, 24577, 512.5]) {
    assert.equal(normalizeReasoningControlForModel("gemini", "gemini-2.5-flash-lite", { type: "budget_tokens", value }), null);
  }
  for (const value of [-1, 512, 24576]) {
    assert.equal(normalizeReasoningControlForModel("gemini", "gemini-2.5-flash-lite", { type: "budget_tokens", value })?.value, value);
  }
});

test("advanced controls require the endpoint's supported protocol and gateway declarations", () => {
  const details = { field_sources: { reasoning: "models_dev" }, reasoning: {
    supported: true, supported_efforts: ["high"], supports_max_tokens: true,
    controls: [{ type: "budget_tokens" as const, min: 128, max: 4096 }],
  } };
  assert.deepEqual(reasoningOptionsForModel("qwen_ai", "qwen3.8-max", details).efforts, []);
  assert.deepEqual(reasoningOptionsForModel("openrouter", "model", details).efforts, []);
  assert.equal(reasoningOptionsForModel("openrouter", "model", details).budget, undefined);
  assert.equal(reasoningOptionsForModel("openai", "model", details).budget, undefined);
  const native = { ...details, field_sources: { reasoning: "provider_api" } };
  assert.deepEqual(reasoningOptionsForModel("openrouter", "model", native).efforts, ["high"]);
  assert.equal(reasoningOptionsForModel("openrouter", "model", native).budget?.min, 128);
  assert.equal(reasoningOptionsForModel("openrouter", "model", {
    ...native, reasoning: { ...native.reasoning, supports_max_tokens: false },
  }).budget, undefined);
  assert.deepEqual(reasoningOptionsForModel("groq", "openai/gpt-oss-safeguard-20b").efforts, []);
  assert.equal(reasoningOptionsForModel("gemini", "gemini-2.5-flash-unknown").budget, undefined);
});

test("stale public subfields do not inherit the trust of native reasoning metadata", () => {
  const details = { stale: true, field_sources: { reasoning: "provider_api", "reasoning.supported_efforts": "models_dev", "reasoning.controls": "models_dev" },
    reasoning: { supported: true, mandatory: true, supported_efforts: ["high"], controls: [{ type: "budget_tokens" as const, min: 128, max: 32768 }] } };
  assert.deepEqual(reasoningOptionsForModel("openrouter", "model", details).efforts, []);
  assert.equal(reasoningOptionsForModel("openrouter", "model", details).budget, undefined);
  assert.equal(normalizeReasoningControlForModel("openrouter", "model", { type: "effort", value: "high" }, details), null);
});

test("OpenRouter controls follow metadata rather than upstream model names", () => {
  assert.equal(thinkingControlForModel("openrouter", "deepseek/reasoner"), "unsupported");
  assert.equal(thinkingControlForModel("openrouter", "model", { input_modalities: ["text"], reasoning: { mandatory: true } }), "hide_only");
  assert.equal(thinkingControlForModel("openrouter", "model", { input_modalities: ["text"], reasoning: { mandatory: false } }), "disable_supported");
  assert.equal(thinkingControlForModel("openrouter", "model", { input_modalities: ["text"], reasoning: {} }), "hide_only");
});

test("public controls apply only to supported protocols and explicit non-reasoning models hide controls", () => {
  const toggle = { reasoning: { supported: true, controls: [{ type: "toggle" as const }] } };
  assert.equal(thinkingControlForModel("alibaba_cloud", "new-model", toggle), "disable_supported");
  assert.equal(thinkingControlForModel("openai_compatible", "new-model", toggle), "unsupported");
  assert.equal(thinkingControlForModel("deepseek", "model", { reasoning: { supported: false } }), "unsupported");
  assert.equal(thinkingControlForModel("openrouter", "model", { reasoning: { mandatory: true, ...toggle.reasoning } }), "hide_only");
  assert.equal(thinkingControlForModel("alibaba_cloud", "new-model", { ...toggle, stale: true }), "unsupported");
});

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

test("native DeepSeek aliases expose effort without a matching catalog entry", () => {
  for (const model of ["deepseek-flash", "deepseek-v4-flash", "deepseek-v4-flash-vision-exp", "deepseek-v4-pro"]) {
    const options = reasoningOptionsForModel("deepseek", model);
    assert.equal(options.thinking, "disable_supported", model);
    assert.deepEqual(options.efforts, ["low", "high", "max"], model);
    for (const value of options.efforts) {
      assert.deepEqual(normalizeReasoningControlForModel("deepseek", model, { type: "effort", value }), { type: "effort", value });
    }
    assert.equal(normalizeReasoningControlForModel("deepseek", model, { type: "effort", value: "medium" }), null);
  }
  assert.deepEqual(reasoningOptionsForModel("deepseek", "deepseek-reasoner").efforts, []);
  assert.deepEqual(reasoningOptionsForModel("openai_compatible", "deepseek-v4-flash").efforts, []);
  assert.deepEqual(reasoningOptionsForModel("deepseek", "deepseek-v4-flash", { reasoning: { supported: false } }).efforts, []);
  assert.deepEqual(reasoningOptionsForModel("deepseek", "deepseek-v4-flash", {
    reasoning: { supported: true, supported_efforts: ["low", "high"] },
  }).efforts, ["low", "high"]);
});

test("custom OpenAI-compatible profiles do not inherit branded parameters", () => {
  assert.equal(
    thinkingControlForModel("openai_compatible", "qwen/qwen3.6-27b"),
    "unsupported",
  );
});

test("Qwen AI exposes the thinking controls supported by its vision models", () => {
  for (const model of ["qwen3.8-max", "qwen3.8-flash", "qwen3-vl-plus", "qwen3-vl-8b-thinking"]) {
    assert.equal(thinkingControlForModel("qwen_ai", model), model.includes("-thinking") ? "hide_only" : "disable_supported", model);
  }
  assert.equal(thinkingControlForModel("qwen_ai", "qwen3-vl-8b-instruct"), "unsupported");
  assert.equal(thinkingControlForModel("qwen_ai", "unknown-model"), "unsupported");
  assert.equal(thinkingControlForModel("qwen_ai", "qwen3.8-flash", { reasoning: { supported: false } }), "unsupported");
  assert.equal(thinkingControlForModel("qwen_ai", "future-model", {
    reasoning: { supported: true, controls: [{ type: "toggle" }] },
  }), "disable_supported");
});

test("Alibaba exposes thinking controls for DeepSeek V4.1 and Vanchin V4 models", () => {
  for (const model of [
    "deepseek-v4.1-flash",
    "vanchin/deepseek-v4-flash",
    "vanchin/deepseek-v4.1-flash",
  ]) {
    assert.equal(thinkingControlForModel("alibaba_cloud", model), "disable_supported");
  }
  assert.equal(thinkingControlForModel("alibaba_cloud", "unknown-model"), "unsupported");
});
