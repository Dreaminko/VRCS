import { useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, ChevronDown, PlusCircle, RefreshCw, Save } from "lucide-react";

import { prefersReducedMotion } from "../../app/app-motion";
import { useLearningCardDraftEditor } from "../hooks/useLearningCardDraftEditor";
import type { LearningWorkspaceController } from "../hooks/useLearningWorkspace";
import type {
  LearningCardType,
  LearningCardDraft,
  LearningItem,
} from "../types";
import { DropdownField } from "../../shared/ui/DropdownField";

export function LearningCardEditor({
  item,
  workspace,
  ankiEnabled,
  ankiAvailable,
  workingText,
  prepareWorkingText,
}: {
  item: LearningItem;
  workspace: LearningWorkspaceController;
  ankiEnabled: boolean;
  ankiAvailable: boolean;
  workingText: string;
  prepareWorkingText: () => Promise<boolean>;
}) {
  const { t } = useTranslation();
  const editor = useLearningCardDraftEditor(item, workspace);
  const { cardType, draft, dirty: draftDirty } = editor;
  const cardRef = useRef<HTMLElement>(null);
  const [generation, setGeneration] = useState(0);
  const hasDraft = Boolean(draft);
  const generating = workspace.busyByItem[item.id] === "draft";
  const busy = workspace.isItemBusy(item.id);
  const disabled = busy || item.status === "archived";
  const workingTextDirty = workingText !== item.working_text;
  const needsSave = draftDirty || workingTextDirty || item.draft_stale;
  const ankiSynced = item.anki_note_id !== null && !item.anki_sync_pending && !needsSave;
  const invalidDraft = !draft?.term.trim() || !draft.definition.trim();
  const exportHint = invalidDraft ? "learning.card.requiredFields"
    : item.draft_stale || workingTextDirty ? "learning.card.stale"
    : draftDirty ? "learning.card.saveBeforeExport"
    : ankiAvailable && !ankiEnabled ? "learning.card.ankiDisabled"
    : "";

  // Replay only for new drafts and successful regeneration, not edits or saves.
  useLayoutEffect(() => {
    if (!hasDraft || !cardRef.current?.animate || prefersReducedMotion()) return;
    const animation = cardRef.current.animate([
      { opacity: 0, transform: "translateY(12px) scale(0.97)" },
      { opacity: 1, transform: "translateY(0) scale(1)" },
    ], { duration: 420, easing: "cubic-bezier(0.34, 1.56, 0.64, 1)" });
    return () => animation.cancel();
  }, [hasDraft, generation]);

  const generate = async () => {
    if (!await prepareWorkingText()) return;
    const generated = await workspace.generateDraft(item.id, cardType);
    if (generated) {
      editor.acceptItem(generated);
      setGeneration((current) => current + 1);
    }
  };
  const save = async () => {
    if (!draft || !await prepareWorkingText()) return;
    const saved = await workspace.saveDraft(item.id, draft);
    if (saved) editor.acceptItem(saved);
  };

  return (
    <section className="learning-card-section" ref={cardRef} aria-label={t("learning.card.title")}>
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
          floating
          label={t("learning.card.type")}
          value={cardType}
          options={cardTypeOptions(t)}
          disabled={disabled}
          onChange={(value) => editor.setCardType(value as LearningCardType)}
        />
        <button
          className={draft ? "secondary-button" : "primary-button"}
          type="button"
          aria-busy={generating}
          disabled={disabled || !workingText.trim()}
          onClick={() => void generate()}
        >
          {generating ? <RefreshCw className="learning-spin" size={15} /> : <PlusCircle size={15} />}
          {t(generating ? "common.loading" : draft ? "learning.card.regenerate" : "learning.card.generate")}
        </button>
      </div>
      {draft ? (
        <>
          <div className="learning-card-action-bar">
            <div className="learning-card-actions">
              <button className={needsSave ? "primary-button" : "secondary-button"} type="button" disabled={disabled || !needsSave || !draft.term.trim() || !workingText.trim()} onClick={() => void save()}>
                <Save size={15} />{t("learning.card.save")}
              </button>
              {ankiAvailable && <button className={needsSave ? "secondary-button" : "primary-button"} type="button" disabled={disabled || needsSave || !ankiEnabled || ankiSynced || invalidDraft} onClick={() => void workspace.exportItem(item.id)}>
                {ankiSynced ? <Check size={15} /> : <PlusCircle size={15} />}
                {t(item.anki_note_id === null ? "learning.card.export" : ankiSynced ? "learning.card.exported" : "learning.card.update")}
              </button>}
            </div>
            {exportHint && <p className="learning-inline-hint" role="status">{t(exportHint)}</p>}
          </div>
          <div className="learning-card-form">
            {cardType === "vocabulary" ? (
              <>
                <CardField label={t("learning.card.term")} value={draft.term} disabled={disabled} onChange={(value) => editor.update("term", value)} />
                <CardField label={t("learning.card.reading")} value={draft.reading ?? ""} disabled={disabled} onChange={(value) => editor.update("reading", value)} />
              </>
            ) : <CardTextArea label={t("learning.card.front")} value={draft.term} disabled={disabled} onChange={(value) => editor.update("term", value)} />}
            <CardTextArea label={t("learning.card.definition")} value={draft.definition} disabled={disabled} onChange={(value) => editor.update("definition", value)} />
          </div>
          <LearningCardPreview draft={draft} />
          <details className="learning-disclosure">
            <summary>{t("learning.card.additionalFields")}<ChevronDown size={15} aria-hidden="true" /></summary>
            <div className="learning-disclosure-body learning-card-form">
              <CardTextArea label={t("learning.card.context")} value={draft.context} disabled={disabled} onChange={(value) => editor.update("context", value)} />
              {cardType !== "vocabulary" && <CardField label={t("learning.card.reading")} value={draft.reading ?? ""} disabled={disabled} onChange={(value) => editor.update("reading", value)} />}
              <CardField label={t("learning.card.language")} value={draft.language ?? ""} disabled={disabled} onChange={(value) => editor.update("language", value)} />
              <CardTextArea label={t("learning.card.dictionary")} value={draft.dictionary ?? ""} disabled={disabled} onChange={(value) => editor.update("dictionary", value)} />
            </div>
          </details>
        </>
      ) : (
        <p className="learning-inline-hint">{t("learning.card.empty")}</p>
      )}
    </section>
  );
}

function LearningCardPreview({ draft }: { draft: LearningCardDraft }) {
  const { t } = useTranslation();
  const [back, setBack] = useState(false);
  return (
    <section className="learning-card-preview" aria-label={t("learning.card.preview")}>
      <div className="learning-section-heading">
        <h3>{t("learning.card.preview")}</h3>
        <div className="learning-tabs" data-active-index={back ? 1 : 0} role="group" aria-label={t("learning.card.preview")}>
          <button type="button" className={!back ? "active" : ""} aria-pressed={!back} onClick={() => setBack(false)}>{t("learning.card.front")}</button>
          <button type="button" className={back ? "active" : ""} aria-pressed={back} onClick={() => setBack(true)}>{t("learning.card.back")}</button>
        </div>
      </div>
      <div className="learning-card-preview-content" key={back ? "back" : "front"}>
        <p className="learning-reading-text">{draft.term}</p>
        {back && <>
          {draft.reading && <p className="learning-card-preview-reading">{draft.reading}</p>}
          <p className="learning-card-preview-definition">{draft.definition}</p>
          {draft.context && draft.context !== draft.term && <div className="learning-source-translation"><span>{t("learning.card.context")}</span><p>{draft.context}</p></div>}
        </>}
      </div>
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
