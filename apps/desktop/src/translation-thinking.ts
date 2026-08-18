export type ThinkingControl = "unsupported" | "hide_only" | "disable_supported";

export function thinkingControlForModel(
  provider: string | undefined,
  model: string,
): ThinkingControl {
  const normalizedModel = model.trim().toLowerCase();
  if (!provider || !normalizedModel) return "unsupported";

  if (provider === "deepseek") return "disable_supported";
  if (provider === "groq") return groqThinkingControl(normalizedModel);
  if (provider === "alibaba_cloud") return alibabaThinkingControl(normalizedModel);
  return "unsupported";
}

function groqThinkingControl(model: string): ThinkingControl {
  if (model === "qwen/qwen3.6-27b") return "disable_supported";
  if (
    model === "openai/gpt-oss-20b"
    || model === "openai/gpt-oss-120b"
    || model === "openai/gpt-oss-safeguard-20b"
    || model === "minimaxai/minimax-m2.7"
  ) return "hide_only";
  return "unsupported";
}

function alibabaThinkingControl(model: string): ThinkingControl {
  if (isAlibabaThinkingOnlyModel(model)) return "hide_only";
  if (isAlibabaHybridModel(model)) return "disable_supported";
  return "unsupported";
}

function isAlibabaThinkingOnlyModel(model: string): boolean {
  return model === "qwq-plus"
    || model === "qwen3.7-max-preview"
    || model === "qwen3.7-max-2026-05-17"
    || model === "qwen3-next-80b-a3b-thinking"
    || model.startsWith("qwen3-235b-a22b-thinking-")
    || model.startsWith("qwen3-30b-a3b-thinking-")
    || model === "deepseek-r1"
    || model.startsWith("deepseek-r1-")
    || model.startsWith("siliconflow/deepseek-r1")
    || model.startsWith("vanchin/deepseek-r1")
    || model === "kimi-k2.7-code"
    || model === "kimi-k2-thinking"
    || model.startsWith("kimi/kimi-k2.7-code")
    || model === "minimax-m2.5"
    || model === "minimax-m2.1";
}

function isAlibabaHybridModel(model: string): boolean {
  return model.startsWith("qwen3.7-max")
    || model.startsWith("qwen3.7-plus")
    || model === "qwen3.6-max-preview"
    || model === "qwen3.6-35b-a3b"
    || model.startsWith("qwen3.6-plus")
    || model.startsWith("qwen3.6-flash")
    || model.startsWith("qwen3.5-plus")
    || model.startsWith("qwen3.5-flash")
    || [
      "qwen3.5-397b-a17b",
      "qwen3.5-122b-a10b",
      "qwen3.5-27b",
      "qwen3.5-35b-a3b",
      "qwen3-235b-a22b",
      "qwen3-32b",
      "qwen3-30b-a3b",
      "qwen3-14b",
      "qwen3-8b",
    ].includes(model)
    || model.startsWith("qwen3-max")
    || model.startsWith("qwen-plus")
    || model.startsWith("qwen-flash")
    || model.startsWith("qwen-turbo")
    || model.startsWith("deepseek-v4-")
    || model.startsWith("deepseek-v3.2")
    || model.startsWith("deepseek-v3.1")
    || model.startsWith("siliconflow/deepseek-v3.")
    || model.startsWith("vanchin/deepseek-v3.")
    || [
      "glm-5.2",
      "glm-5.2-us",
      "glm-5.2-fast-preview",
      "glm-5.1",
      "glm-5",
      "glm-4.7",
      "glm-4.6",
      "glm-4.5",
      "glm-4.5-air",
      "kimi-k2.6",
      "kimi-k2.5",
      "kimi/kimi-k2.6",
      "kimi/kimi-k2.5",
    ].includes(model);
}
