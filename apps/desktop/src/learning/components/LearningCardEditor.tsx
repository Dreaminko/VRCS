import { useTranslation } from "react-i18next";
import { Check, PlusCircle, Save } from "lucide-react";

import { useLearningCardDraftEditor } from "../hooks/useLearningCardDraftEditor";
import type { LearningWorkspaceController } from "../hooks/useLearningWorkspace";
import type {
  LearningCardType,
  LearningItem,
} from "../types";
import { DropdownField } from "../../shared/ui/DropdownField";

export function LearningCardEditor({
  item,
  workspace,
  ankiEnabled,
  workingText,
  prepareWorkingText,
}: {
  item: LearningItem;
  workspace: LearningWorkspaceController;
  ankiEnabled: boolean;
  workingText: string;
  prepareWorkingText: () => Promise<boolean>;
}) {
  const { t } = useTranslation();
  const editor = useLearningCardDraftEditor(item);
  const { cardType, draft, dirty: draftDirty } = editor;
  const busy = workspace.isItemBusy(item.id);
  const disabled = busy || item.status === "archived";

  const generate = async () => {
    if (!await prepareWorkingText()) return;
    const generated = await workspace.generateDraft(item.id, cardType);
    if (generated) editor.acceptItem(generated);
  };
  const save = async () => {
    if (!draft) return;
    const saved = await workspace.saveDraft(item.id, draft);
    if (saved) editor.acceptItem(saved);
  };

  return (
    <section className="learning-card-section" aria-label={t("learning.card.title")}>
      <div className="learning-section-heading">
        <div>
          <h3>{t("learning.card.title")}</h3>
          <p>{t("learning.card.description")}</p>
        </div>
        {item.anki_note_id !== null && <span><Check size={13} />{t("learning.card.noteId", { id: item.anki_note_id })}</span>}
      </div>
      <div className="learning-card-toolbar">
        <DropdownField
          compact
          label={t("learning.card.type")}
          value={cardType}
          options={cardTypeOptions(t)}
          disabled={disabled}
          onChange={(value) => editor.setCardType(value as LearningCardType)}
        />
        <button
          className="secondary-button"
          type="button"
          disabled={disabled || !workingText.trim()}
          onClick={() => void generate()}
        >
          <PlusCircle size={15} />
          {t(item.draft ? "learning.card.regenerate" : "learning.card.generate")}
        </button>
      </div>
      {draft ? (
        <div className="learning-card-form">
          <CardField label={t("learning.card.term")} value={draft.term} disabled={disabled} onChange={(value) => editor.update("term", value)} />
          <CardField label={t("learning.card.reading")} value={draft.reading ?? ""} disabled={disabled} onChange={(value) => editor.update("reading", value)} />
          <CardTextArea label={t("learning.card.definition")} value={draft.definition} disabled={disabled} onChange={(value) => editor.update("definition", value)} />
          <CardTextArea label={t("learning.card.context")} value={draft.context} disabled={disabled} onChange={(value) => editor.update("context", value)} />
          <CardTextArea label={t("learning.card.dictionary")} value={draft.dictionary ?? ""} disabled={disabled} onChange={(value) => editor.update("dictionary", value)} />
          <CardField label={t("learning.card.language")} value={draft.language ?? ""} disabled={disabled} onChange={(value) => editor.update("language", value)} />
          <div className="learning-card-actions">
            <button className="secondary-button" type="button" disabled={disabled || !draftDirty || !draft.term.trim() || !draft.definition.trim()} onClick={() => void save()}>
              <Save size={15} />{t("learning.card.save")}
            </button>
            <button className="primary-button" type="button" disabled={disabled || draftDirty || !ankiEnabled || item.anki_note_id !== null || !draft.term.trim() || !draft.definition.trim()} onClick={() => void workspace.exportItem(item.id)}>
              {item.anki_note_id === null ? <PlusCircle size={15} /> : <Check size={15} />}
              {t(item.anki_note_id === null ? "learning.card.export" : "learning.card.exported")}
            </button>
          </div>
          {draftDirty && <p className="learning-inline-hint">{t("learning.card.saveBeforeExport")}</p>}
          {!ankiEnabled && <p className="learning-inline-hint">{t("learning.card.ankiDisabled")}</p>}
        </div>
      ) : (
        <p className="learning-inline-hint">{t("learning.card.empty")}</p>
      )}
    </section>
  );
}

function CardField({ label, value, disabled, onChange }: { label: string; value: string; disabled: boolean; onChange: (value: string) => void }) {
  return (
    <label className="learning-field">
      <span>{label}</span>
      <input value={value} disabled={disabled} onChange={(event) => onChange(event.target.value)} />
    </label>
  );
}

function CardTextArea({ label, value, disabled, onChange }: { label: string; value: string; disabled: boolean; onChange: (value: string) => void }) {
  return (
    <label className="learning-field learning-field-wide">
      <span>{label}</span>
      <textarea value={value} disabled={disabled} onChange={(event) => onChange(event.target.value)} />
    </label>
  );
}

function cardTypeOptions(t: (key: string) => string) {
  return [
    { value: "vocabulary", label: t("learning.card.types.vocabulary") },
    { value: "sentence", label: t("learning.card.types.sentence") },
    { value: "fill_blank", label: t("learning.card.types.fillBlank") },
  ];
}
