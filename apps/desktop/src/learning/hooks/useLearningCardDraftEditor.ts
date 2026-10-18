import { learningItemEditor } from "../learning-edits";
import type { LearningWorkspaceController } from "./useLearningWorkspace";
import type {
  LearningCardDraft,
  LearningCardType,
  LearningItem,
} from "../types";

export function useLearningCardDraftEditor(item: LearningItem, workspace: LearningWorkspaceController) {
  const { cardType, draft, dirty } = learningItemEditor(item, workspace.itemEdits[item.id]);
  const setCardType = (next: LearningCardType) => {
    workspace.editItem(item.id, { cardType: next, draft: draft ? { ...draft, card_type: next } : undefined });
  };
  const update = <Key extends keyof LearningCardDraft>(
    key: Key,
    value: LearningCardDraft[Key],
  ) => {
    if (draft) workspace.editItem(item.id, { draft: { ...draft, [key]: value } });
  };
  const acceptItem = (next: LearningItem) => {
    workspace.editItem(next.id, { cardType: undefined, draft: undefined });
  };

  return { cardType, draft, dirty, setCardType, update, acceptItem };
}
