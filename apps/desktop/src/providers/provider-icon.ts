import type { IconType } from "@lobehub/icons/es/types";
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

export interface ProviderIconInput {
  provider: string;
}

// Color marks carry the official brand palette. Mono marks inherit currentColor.
const PROVIDER_ICONS: Record<string, IconType> = {
  openai: OpenAI,
  gemini: Gemini,
  groq: Groq,
  deepseek: DeepSeek,
  mimo: XiaomiMiMo,
  openrouter: OpenRouter,
  alibaba_cloud: AlibabaCloud,
  alibaba_token_plan: AlibabaCloud,
  qwen_ai: Qwen,
  qwen_local: Qwen,
  lm_studio: LmStudio,
  ollama: Ollama,
  deepl: DeepL,
  microsoft_translator: Microsoft,
};

export function providerIcon({ provider }: ProviderIconInput): IconType | null {
  return PROVIDER_ICONS[provider] ?? null;
}
