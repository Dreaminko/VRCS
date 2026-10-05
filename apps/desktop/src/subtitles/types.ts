import type { ApiProvider } from "../providers/types";

export interface SpeakerIdentity {
  id: string;
  index: number;
}

export interface Subtitle {
  /** Runtime link from the subtitle event; not stored in history. */
  utterance_id?: string;
  speaker?: SpeakerIdentity | null;
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
  source_group?: { subtitle_ids: number[]; text: string };
  text: string;
  source_language: string | null;
  target_language: string;
  provider: ApiProvider | "local";
  model: string | null;
  created_at: string;
}

export interface ConversationSubtitleContext {
  items: Subtitle[];
  target_id: number;
  has_older: boolean;
}

export interface SubtitleSearchHit {
  subtitle: Subtitle;
  matched_field: "original" | "translation";
  matched_text: string;
}

export interface SubtitleSearchPage {
  items: SubtitleSearchHit[];
  has_more: boolean;
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
