import type { ApiModelDetails } from "../providers/types";

export type ThinkingControl = "unsupported" | "hide_only" | "disable_supported";

export type ReasoningSetting = { type: "effort"; value: string } | { type: "budget_tokens"; value: number };

function currentField(details: ApiModelDetails | undefined, field: string): boolean {
  if (!details?.stale) return true;
  const source = details.field_sources?.[field] ?? details.field_sources?.reasoning;
  return source === "provider_api" || source === "builtin";
}

export function reasoningOptionsForModel(provider: string | undefined, model: string, details?: ApiModelDetails) {
  const thinking = thinkingControlForModel(provider, model, details);
  const options = details?.reasoning;
  const implemented = ["openai", "gemini", "deepseek", "groq", "alibaba_cloud", "alibaba_token_plan", "openrouter"].includes(provider ?? "");
  if (!implemented || (currentField(details, "reasoning") && options?.supported === false)) {
    return { thinking, efforts: [] as string[], budget: undefined };
  }
  const confirmed = (field: string) => currentField(details, field) && (provider !== "openrouter"
    || (details?.field_sources?.[field] ?? details?.field_sources?.reasoning) === "provider_api");
  const efforts = confirmed("reasoning.supported_efforts")
    ? options?.supported_efforts ?? [] : [];
  const effortControl = confirmed("reasoning.supported_efforts") && confirmed("reasoning.controls")
    ? options?.controls?.find((control) => control.type === "effort") : undefined;
  const values = efforts.length ? efforts : effortControl?.type === "effort" ? effortControl.values : [];
  const supportsBudget = ["gemini", "alibaba_cloud", "alibaba_token_plan"].includes(provider ?? "")
    || (provider === "openrouter" && confirmed("reasoning.supports_max_tokens") && options?.supports_max_tokens === true);
  const budgetControl = supportsBudget && confirmed("reasoning.controls")
    ? options?.controls?.find((control) => control.type === "budget_tokens") : undefined;
  let budget = budgetControl?.type === "budget_tokens" ? {
    min: Math.max(1, budgetControl.min ?? 1), max: budgetControl.max,
    dynamic: provider === "gemini" && confirmed("reasoning.supports_dynamic_budget") && options?.supports_dynamic_budget === true,
  } : undefined;
  let available = [...new Set(values.filter((value) => value !== "none"))];
  if (!details?.reasoning && provider === "deepseek"
    && ["deepseek-flash", "deepseek-v4-flash", "deepseek-v4-flash-vision-exp", "deepseek-v4-pro"].includes(model)) {
    available = ["low", "high", "max"];
  }
  if (!details?.reasoning && provider === "groq" && /^openai\/gpt-oss-(20b|120b)$/.test(model)) {
    available = ["low", "medium", "high"];
  }
  if (!details?.reasoning && provider === "groq" && model === "qwen/qwen3.8-27b") available = ["default", "low", "medium", "high"];
  if (!details?.reasoning && provider === "groq" && model === "qwen/qwen3.6-27b") available = ["default"];
  if (!details?.reasoning && provider === "gemini" && ["gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.5-flash-lite"].includes(model)) {
    const pro = model === "gemini-2.5-pro";
    budget = { min: pro ? 128 : model === "gemini-2.5-flash-lite" ? 512 : 1, max: pro ? 32768 : 24576, dynamic: true };
  }
  return { thinking, efforts: available, budget };
}

export function normalizeReasoningControlForModel(
  provider: string | undefined, model: string, value: ReasoningSetting | null | undefined, details?: ApiModelDetails,
): ReasoningSetting | null {
  if (!value) return null;
  const options = reasoningOptionsForModel(provider, model, details);
  if (value.type === "effort") return options.efforts.includes(value.value) ? value : null;
  const budget = options.budget;
  return budget && Number.isSafeInteger(value.value)
    && (value.value === -1 ? budget.dynamic : value.value >= budget.min && (budget.max == null || value.value <= budget.max))
    ? value : null;
}

export function thinkingControlForModel(
  provider: string | undefined,
  model: string,
  details?: ApiModelDetails,
): ThinkingControl {
  const normalizedModel = model.trim().toLowerCase();
  if (!provider || !normalizedModel) return "unsupported";
  if (provider === "openai_compatible") return "unsupported";
  const currentReasoning = currentField(details, "reasoning");
  if (currentReasoning && details?.reasoning?.supported === false) return "unsupported";
  if (currentField(details, "reasoning.can_disable") && details?.reasoning?.can_disable != null) {
    return details.reasoning.can_disable ? "disable_supported" : "hide_only";
  }
  if (provider === "openrouter") {
    if (!currentReasoning || !details?.reasoning) return "unsupported";
    return details.reasoning.mandatory === false ? "disable_supported" : "hide_only";
  }
  if (currentReasoning && details?.reasoning?.supported === true) {
    const options = details.reasoning;
    if (options.mandatory === true) return "hide_only";
    if (["alibaba_cloud", "alibaba_token_plan", "qwen_ai", "deepseek"].includes(provider)
      && currentField(details, "reasoning.controls")
      && options.controls?.some((control) => control.type === "toggle")) return "disable_supported";
    if (["openai", "gemini", "groq"].includes(provider) && options.supported_efforts?.length
      && currentField(details, "reasoning.supported_efforts")) {
      return provider !== "gemini" && options.supported_efforts.includes("none") ? "disable_supported" : "hide_only";
    }
  }

  if (provider === "deepseek") return "disable_supported";
  if (provider === "mimo") {
    return ["mimo-v2.6-flash", "mimo-v2.6-pro", "mimo-v2.6-pro-ultraspeed"].includes(normalizedModel)
      ? "disable_supported" : "unsupported";
  }
  if (provider === "gemini" && normalizedModel.startsWith("gemini-2.5-flash")) return "disable_supported";
  if (provider === "gemini" && normalizedModel.startsWith("gemini-2.5-pro")) return "hide_only";
  if (provider === "groq") return groqThinkingControl(normalizedModel);
  if (provider === "alibaba_cloud" || provider === "alibaba_token_plan" || provider === "qwen_ai") {
    return alibabaThinkingControl(normalizedModel);
  }
  return "unsupported";
}

function groqThinkingControl(model: string): ThinkingControl {
  if (model === "qwen/qwen3.6-27b" || model === "qwen/qwen3.8-27b") return "disable_supported";
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
  return (model.startsWith("qwen3-vl-") && model.includes("-thinking"))
    || model === "qwq-plus"
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
  return model.startsWith("qwen3.8-max")
    || model.startsWith("qwen3.8-flash")
    || model.startsWith("qwen3-vl-plus")
    || model.startsWith("qwen3.7-max")
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
    || model.startsWith("deepseek-v4.1-")
    || model.startsWith("vanchin/deepseek-v4-")
    || model.startsWith("vanchin/deepseek-v4.1-")
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
