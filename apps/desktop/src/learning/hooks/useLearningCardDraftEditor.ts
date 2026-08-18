import { useCallback, useEffect, useRef, useState } from "react";

import { normalizeLearningCardDraft } from "../../learning";
import type { LearningCardDraft, LearningCardType, LearningItem } from "../../types";

export function useLearningCardDraftEditor(item: LearningItem) {
  const [cardType, setCardTypeState] = useState<LearningCardType>(() => initialCardType(item));
  const [draft, setDraft] = useState<LearningCardDraft | null>(item.draft);
  const [savedDraft, setSavedDraft] = useState<LearningCardDraft | null>(item.draft);
  const itemIdRef = useRef(item.id);
  const dirty = !sameDraft(draft, savedDraft);
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;

  useEffect(() => {
    if (itemIdRef.current !== item.id) {
      itemIdRef.current = item.id;
      setCardTypeState(initialCardType(item));
      setDraft(item.draft);
      setSavedDraft(item.draft);
      return;
    }
    if (!dirtyRef.current) {
      setCardTypeState(initialCardType(item));
      setDraft(item.draft);
      setSavedDraft(item.draft);
    }
  }, [item.id, item.draft]);

  const setCardType = useCallback((next: LearningCardType) => {
    setCardTypeState(next);
    setDraft((current) => current ? { ...current, card_type: next } : current);
  }, []);

  const update = useCallback(<Key extends keyof LearningCardDraft>(
    key: Key,
    value: LearningCardDraft[Key],
  ) => {
    setDraft((current) => current ? { ...current, [key]: value } : current);
  }, []);

  const acceptItem = useCallback((next: LearningItem) => {
    if (next.id !== itemIdRef.current) return;
    setCardTypeState(initialCardType(next));
    setDraft(next.draft);
    setSavedDraft(next.draft);
  }, []);

  return { cardType, draft, dirty, setCardType, update, acceptItem };
}

function initialCardType(item: LearningItem): LearningCardType {
  return item.draft?.card_type ?? (item.kind === "word" ? "vocabulary" : "sentence");
}

function sameDraft(
  left: LearningCardDraft | null,
  right: LearningCardDraft | null,
): boolean {
  if (left === null || right === null) return left === right;
  return JSON.stringify(normalizeLearningCardDraft(left))
    === JSON.stringify(normalizeLearningCardDraft(right));
}
