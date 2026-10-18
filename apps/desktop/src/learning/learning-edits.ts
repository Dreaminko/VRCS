import { normalizeLearningCardDraft } from "./learning.ts";
import type { LearningCardDraft, LearningCardType, LearningItem } from "./types.ts";

export interface LearningItemEdits {
  workingText?: string;
  cardType?: LearningCardType;
  draft?: LearningCardDraft;
}

export function updateLearningItemEdits(
  edits: Record<number, LearningItemEdits>,
  itemId: number,
  patch: LearningItemEdits,
): Record<number, LearningItemEdits> {
  return { ...edits, [itemId]: { ...edits[itemId], ...patch } };
}

export function learningItemEditor(item: LearningItem, edits?: LearningItemEdits) {
  const draft = edits?.draft ?? item.draft;
  const dirty = JSON.stringify(draft && normalizeLearningCardDraft(draft))
    !== JSON.stringify(item.draft && normalizeLearningCardDraft(item.draft));
  return {
    workingText: edits?.workingText ?? item.working_text,
    cardType: edits?.cardType ?? draft?.card_type ?? (item.kind === "word" ? "vocabulary" : "sentence"),
    draft,
    dirty,
  };
}
