import type { ApiProfileView } from "./providers/types";

type ModelCatalog = {
  models: string[];
  loading: boolean;
  error: string;
};

const PREFERRED_MODELS: Record<string, readonly string[]> = {
  openai: ["gpt-4.1-mini", "gpt-4o-mini", "gpt-5-mini"],
  groq: ["openai/gpt-oss-20b", "openai/gpt-oss-120b"],
  deepseek: ["deepseek-v4-flash", "deepseek-v4-pro"],
  gemini: ["gemini-3.7-flash", "gemini-3.6-flash", "gemini-2.5-flash"],
  alibaba_cloud: ["qwen3.6-flash", "qwen3.7-plus", "qwen3.7-max"],
  openrouter: ["openai/gpt-5-mini", "google/gemini-2.5-flash", "openai/gpt-4o-mini"],
};

const NON_TEXT_MODEL_MARKERS = [
  "embedding",
  "rerank",
  "whisper",
  "transcribe",
  "tts",
  "speech",
  "realtime",
  "audio",
  "image",
  "guard",
  "moderation",
] as const;

function preferredModel(provider: string, models: readonly string[]): string | undefined {
  return PREFERRED_MODELS[provider]?.find((candidate) => models.includes(candidate));
}

function likelyTextModel(model: string): boolean {
  const normalized = model.toLowerCase();
  return !normalized.endsWith(":batch")
    && !NON_TEXT_MODEL_MARKERS.some((marker) => normalized.includes(marker));
}

export function selectTranslationModel(
  provider: string,
  models: readonly string[],
  currentModel: string,
): string | undefined {
  if (models.includes(currentModel) && likelyTextModel(currentModel)) return currentModel;
  const preferred = preferredModel(provider, models);
  if (preferred) return preferred;
  return models.find(likelyTextModel);
}

export function translationDiagnosticModel(
  profile: ApiProfileView,
  catalog: ModelCatalog | undefined,
  configuredModel: string,
): string | undefined {
  const model = configuredModel.trim();
  if (!model) return undefined;
  if (!profile.capabilities.supports_model_listing) return model;
  if (catalog?.error) return model;
  if (!catalog || catalog.loading) {
    return profile.provider === "openai_compatible" ? model : undefined;
  }
  return catalog.models.includes(model) ? model : undefined;
}
