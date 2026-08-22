import type { DictionaryEntry } from "../dictionary/types";

export type LearningItemKind = "word" | "sentence" | "excerpt";
export type LearningItemStatus = "collected" | "analyzed" | "card_draft" | "exported" | "archived";
export type LearningCardType = "vocabulary" | "sentence" | "fill_blank";
export type LearningTaskType = "contextual_word_explanation" | "sentence_analysis" | "session_review";
export type LearningLevel = "beginner" | "intermediate" | "advanced";
export type LearningAnalysisFocus = "default" | "simpler" | "examples" | "compare";
export type LearningAnalysisConfidence = "low" | "medium" | "high";

export interface LearningAnalysisSegment {
  text: string;
  role: string;
  explanation: string;
}

export interface LearningGrammarPoint {
  form: string;
  meaning: string;
  note: string;
}

export interface LearningExample {
  source: string;
  translation: string;
}

export interface LearningAnalysis {
  task_type: LearningTaskType;
  summary: string;
  current_meaning?: string | null;
  base_form?: string | null;
  part_of_speech?: string | null;
  register?: string | null;
  segments: LearningAnalysisSegment[];
  grammar_points: LearningGrammarPoint[];
  uncertainties: string[];
  memory_tip?: string | null;
  examples: LearningExample[];
  confidence: LearningAnalysisConfidence;
  provider: string;
  model: string;
  prompt_version: string;
}

export interface LearningCardDraft {
  card_type: LearningCardType;
  term: string;
  reading?: string | null;
  definition: string;
  context: string;
  dictionary?: string | null;
  language?: string | null;
}

export interface LearningItem {
  id: number;
  kind: LearningItemKind;
  status: LearningItemStatus;
  source_text: string;
  working_text: string;
  selected_text: string | null;
  source_translation: string | null;
  source_language: string | null;
  source_subtitle_ids: number[];
  dictionary_entries: DictionaryEntry[];
  analysis: LearningAnalysis | null;
  draft: LearningCardDraft | null;
  anki_note_id: number | null;
  created_at: string;
  updated_at: string;
}

export interface LearningItemCreateInput {
  kind: LearningItemKind;
  source_text: string;
  working_text: string;
  selected_text: string | null;
  source_translation: string | null;
  source_language: string | null;
  source_subtitle_ids: number[];
  dictionary_entries: DictionaryEntry[];
}

export interface LearningItemPatchInput {
  working_text?: string;
  draft?: LearningCardDraft | null;
}

export interface LearningAnalysisInput {
  task_type: LearningTaskType;
  focus?: LearningAnalysisFocus;
  profile_id: string;
  model: string;
  explanation_language: string;
  level: LearningLevel;
}

export interface SelectionQueryInput {
  selected_text: string;
  source_text: string;
  source_translation: string | null;
  source_language: string | null;
  question: string;
  profile_id: string;
  model: string;
  explanation_language: string;
  level: LearningLevel;
}

export interface SelectionQueryResponse {
  answer: string;
  provider: string;
  model: string;
  prompt_version: string;
}

export interface LearningDraftInput {
  card_type: LearningCardType;
}
