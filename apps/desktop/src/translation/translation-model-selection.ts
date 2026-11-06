import type { ApiModelDetails, ApiProfileView } from "../providers/types";
import type { TranslationTargetSettings } from "../settings/types";

const PREFERRED_MODELS: Record<string, readonly string[]> = {
  openai: ["gpt-4.1-mini", "gpt-4o-mini", "gpt-5-mini"],
  groq: ["openai/gpt-oss-20b", "openai/gpt-oss-120b"],
  deepseek: ["deepseek-v4-flash", "deepseek-v4-pro"],
  mimo: ["mimo-v2.6-flash", "mimo-v2.6-pro"],
  gemini: ["gemini-3.7-flash", "gemini-3.6-flash", "gemini-2.5-flash"],
  alibaba_cloud: ["qwen3.6-flash", "qwen3.7-plus", "qwen3.7-max"],
  alibaba_token_plan: ["qwen3.6-flash", "qwen3.7-plus", "qwen3.7-max"],
  openrouter: ["openai/gpt-4o-mini", "google/gemini-2.5-flash", "openai/gpt-5-mini"],
};

const NON_TEXT_MODEL_MARKERS = [
  "embed",
  "rerank",
  "whisper",
  "transcri",
  "asr",
  "tts",
  "speech",
  "orpheus",
  "realtime",
  "audio",
  "image",
  "guard",
  "moderation",
] as const;

function preferredModel(provider: string, models: readonly string[]): string | undefined {
  return PREFERRED_MODELS[provider]?.find((candidate) => models.includes(candidate));
}

function likelyTextModel(model: string, details?: ApiModelDetails): boolean {
  if (!model.trim()) return false;
  if (details?.input_modalities?.length && !details.input_modalities.includes("text")) return false;
  if (details?.output_modalities?.length && !details.output_modalities.includes("text")) return false;
  if (details?.input_modalities?.includes("text") && details.output_modalities?.includes("text")) return true;
  const normalized = model.toLowerCase();
  return !normalized.endsWith(":batch")
    && !NON_TEXT_MODEL_MARKERS.some((marker) => normalized.includes(marker));
}

export function selectTranslationModel(
  provider: string,
  models: readonly string[],
  currentModel: string,
  modelDetails: Record<string, ApiModelDetails> = {},
): string | undefined {
  if (models.includes(currentModel) && likelyTextModel(currentModel, modelDetails[currentModel])) return currentModel;
  const candidates = models.filter((model) => likelyTextModel(model, modelDetails[model]));
  const preferred = preferredModel(provider, candidates);
  if (preferred) return preferred;
  return candidates[0];
}

export function translationModels(models: readonly string[], details: Record<string, ApiModelDetails> = {}): string[] {
  return models.filter((model) => likelyTextModel(model, details[model]));
}

export function updateTranslationModel(
  target: TranslationTargetSettings,
  model: string,
): TranslationTargetSettings {
  return {
    ...target,
    model,
    ...(target.profile_id ? {
      model_by_profile: { ...target.model_by_profile, [target.profile_id]: model },
    } : {}),
  };
}

export function switchTranslationProfile(
  target: TranslationTargetSettings,
  profile: ApiProfileView,
  models: readonly string[] = [],
  modelDetails: Record<string, ApiModelDetails> = {},
): TranslationTargetSettings {
  const remembered = updateTranslationModel(target, target.model).model_by_profile;
  const savedModel = Object.hasOwn(remembered ?? {}, profile.id) ? remembered?.[profile.id] : undefined;
  const model = savedModel ?? selectTranslationModel(profile.provider, models, target.model, modelDetails) ?? "";
  return updateTranslationModel({ ...target, profile_id: profile.id, model_by_profile: remembered }, model);
}
