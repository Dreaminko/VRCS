import assert from "node:assert/strict";
import test from "node:test";

import { learningItemEditor, updateLearningItemEdits } from "../src/learning/learning-edits.ts";
import type { LearningItemEdits } from "../src/learning/learning-edits.ts";
import type { LearningItem } from "../src/learning/types.ts";

function item(id: number): LearningItem {
  return {
    id, kind: "sentence", status: "card_draft", source_text: "Original", working_text: "Original",
    selected_text: null, source_translation: null, source_language: "en", source_subtitle_ids: [],
    dictionary_entries: [], analysis: null, analysis_stale: false, draft_stale: false,
    draft: { card_type: "sentence", term: "Original", definition: "Meaning", context: "Original" },
    anki_note_id: null, anki_sync_pending: false, created_at: "", updated_at: "",
  };
}

test("restores unsaved text and draft edits independently for each item", () => {
  const first = item(1);
  const second = item(2);
  let edits: Record<number, LearningItemEdits> = {};
  const draft = { ...first.draft!, definition: "Edited meaning", card_type: "fill_blank" as const };
  edits = updateLearningItemEdits(edits, first.id, { workingText: "Edited text", draft, cardType: "fill_blank" });
  edits = updateLearningItemEdits(edits, second.id, { workingText: "Second edit" });

  assert.equal(learningItemEditor(second, edits[second.id]).workingText, "Second edit");
  const restored = learningItemEditor(first, edits[first.id]);
  assert.equal(restored.workingText, "Edited text");
  assert.deepEqual(restored.draft, draft);
  assert.equal(restored.cardType, "fill_blank");
  assert.equal(restored.dirty, true);
});

test("accepting a saved field preserves the other unsaved fields and items", () => {
  const first = item(1);
  let edits = updateLearningItemEdits({}, first.id, {
    workingText: "Edited text", draft: { ...first.draft!, definition: "Edited meaning" },
  });
  edits = updateLearningItemEdits(edits, 2, { workingText: "Second edit" });
  edits = updateLearningItemEdits(edits, first.id, { workingText: undefined });
  const saved = { ...first, working_text: "Edited text" };
  const editor = learningItemEditor(saved, edits[first.id]);
  assert.equal(editor.workingText, "Edited text");
  assert.equal(editor.draft?.definition, "Edited meaning");
  assert.equal(editor.dirty, true);
  edits = updateLearningItemEdits(edits, first.id, { draft: undefined, cardType: undefined });
  assert.equal(learningItemEditor(saved, edits[first.id]).dirty, false);
  assert.equal(edits[2].workingText, "Second edit");
});

test("untouched fields follow server updates while unsaved changes remain", () => {
  const first = item(1);
  const edits = updateLearningItemEdits({}, 1, { workingText: "Local text" });
  const updated = { ...first, draft: { ...first.draft!, definition: "Server meaning" } };
  const editor = learningItemEditor(updated, edits[1]);
  assert.equal(editor.workingText, "Local text");
  assert.equal(editor.draft?.definition, "Server meaning");
  assert.equal(editor.dirty, false);
});
