import type { ApiProvider } from "../providers/types";

export interface Subtitle {
  id: number | null;
  text: string;
  language: string | null;
  started_at: number | null;
  ended_at: number | null;
  created_at: string;
  conversation_id?: string | null;
  source?: "speaker" | "microphone" | "chatbox";
  translations: SubtitleTranslation[];
  translation_partial?: {
    text: string;
    target_language: string;
  };
}

export interface SubtitleTranslation {
  text: string;
  source_language: string | null;
  target_language: string;
  provider: ApiProvider | "local";
  model: string | null;
  created_at: string;
}

export interface TranslationEvent {
  type: "translation_started" | "translation_partial" | "translation_completed" | "translation_failed";
  subtitle_id: number;
  text?: string;
  target_language?: string;
  translation?: SubtitleTranslation;
  code?: string;
  detail?: string;
  preferred?: boolean;
}
