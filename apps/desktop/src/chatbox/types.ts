export type ChatboxSendMode = "original" | "translation" | "bilingual";
export type ChatboxMessageFormat =
  | "original_newline_translation"
  | "translation_newline_original"
  | "slash_separated"
  | "custom";
export type ChatboxOverflowPolicy = "block" | "smart_truncate";

export interface ChatboxComposeInput {
  original: string;
  translation: string | null;
  source_language: string | null;
  target_language: string | null;
  send_mode: ChatboxSendMode;
  message_format: ChatboxMessageFormat;
  custom_format: string | null;
  overflow_policy: ChatboxOverflowPolicy;
}

export interface ChatboxPreview {
  text: string;
  char_count: number;
  limit: number;
  over_limit: boolean;
  truncated: boolean;
  sendable: boolean;
}

export interface ChatboxMessage {
  id: number;
  source: "manual" | "microphone" | "resend";
  original: string;
  translation: string | null;
  source_language: string | null;
  target_language: string | null;
  send_mode: ChatboxSendMode;
  message_format: ChatboxMessageFormat;
  custom_format: string | null;
  rendered_text: string;
  char_count: number;
  truncated: boolean;
  status: "sent" | "failed";
  error_code: string | null;
  error_detail: string | null;
  resent_from_id: number | null;
  created_at: string;
  sent_at: string | null;
}
