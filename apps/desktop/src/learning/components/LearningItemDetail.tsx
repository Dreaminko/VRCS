import { useRef, useState, type RefObject, type UIEventHandler } from "react";
import { useTranslation } from "react-i18next";
import { Archive, ArrowLeft, BookOpenText, ChevronDown, CreditCard, MoreHorizontal, Pencil, RefreshCw, RotateCcw, Save, Sparkles, Trash2 } from "lucide-react";

import { ConfirmDialog } from "../../app/components/ConfirmDialog";
import { useDismissibleLayer } from "../../shared/hooks/use-dismissible-layer";
import { useDropdownMenuMotion } from "../../shared/hooks/use-dropdown-menu-motion";
import { DropdownField } from "../../shared/ui/DropdownField";
import type { LearningWorkspaceController } from "../hooks/useLearningWorkspace";
import { learningTaskForKind } from "../learning";
import { learningItemEditor } from "../learning-edits";
import type { LearningAnalysisFocus, LearningItem, LearningTaskType } from "../types";
import { LearningAnalysisView } from "./LearningAnalysisView";
import { LearningCardEditor } from "./LearningCardEditor";

export function LearningItemDetail({
  item,
  workspace,
  ankiEnabled,
  ankiAvailable,
  view,
  onViewChange,
  onBack,
  onOpenAiSettings,
  scrollRef,
  onScroll,
}: {
  item: LearningItem;
  workspace: LearningWorkspaceController;
  ankiEnabled: boolean;
  ankiAvailable: boolean;
  view: "study" | "card";
  onViewChange: (view: "study" | "card") => void;
  onBack: () => void;
  onOpenAiSettings: () => void;
  scrollRef: RefObject<HTMLDivElement | null>;
  onScroll: UIEventHandler<HTMLDivElement>;
}) {
  const { t } = useTranslation();
  const { workingText, cardType, draft } = learningItemEditor(item, workspace.itemEdits[item.id]);
  const [editing, setEditing] = useState(workingText !== item.working_text);
  const [task, setTask] = useState(item.analysis?.task_type ?? learningTaskForKind(item.kind));
  const [managementOpen, setManagementOpen] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const managementRef = useRef<HTMLDivElement>(null);
  const managementButtonRef = useRef<HTMLButtonElement>(null);
  const { menuRef: managementMenuRef, mounted: managementMenuMounted } = useDropdownMenuMotion(managementOpen);
  useDismissibleLayer(managementOpen, managementRef, () => setManagementOpen(false));

  const busy = workspace.isItemBusy(item.id);
  const archived = item.status === "archived";
  const workingTextDirty = workingText !== item.working_text;
  const canUseAi = Boolean(workspace.preferences.profileId && workspace.preferences.model.trim());
  const analysisDisabled = busy || archived || !canUseAi || !workingText.trim();
  const analyzing = workspace.busyByItem[item.id] === "analysis";
  const generating = workspace.busyByItem[item.id] === "draft";
  const textLabel = t(`learning.detail.textLabels.${item.kind}`);
  const selectedTerm = item.selected_text?.trim();
  const importedEntries = item.dictionary_entries.filter((entry) => Boolean(entry.dictionary?.trim()));
  const primaryEntry = importedEntries.find((entry) => entry.term.trim() === selectedTerm)
    ?? importedEntries[0]
    ?? item.dictionary_entries.find((entry) => entry.term.trim() === selectedTerm)
    ?? item.dictionary_entries[0];
  const otherEntries = item.dictionary_entries.filter((entry) => entry !== primaryEntry);
  const collapsedEntries = item.kind === "word" ? otherEntries : item.dictionary_entries;

  const prepareWorkingText = async (): Promise<boolean> => {
    if (archived) return false;
    if (!workingTextDirty) return true;
    const saved = await workspace.updateWorkingText(item.id, workingText);
    if (saved) workspace.editItem(item.id, { workingText: undefined });
    return Boolean(saved);
  };
  const analyze = async (taskType: LearningTaskType, focus?: LearningAnalysisFocus) => {
    if (analysisDisabled || !await prepareWorkingText()) return;
    await workspace.analyze(item.id, taskType, focus);
  };
  const openCard = async () => {
    if (!draft) {
      if (busy || archived || !await prepareWorkingText()) return;
      const generated = await workspace.generateDraft(item.id, cardType);
      if (!generated) return;
      workspace.editItem(item.id, { cardType: undefined, draft: undefined });
    }
    onViewChange("card");
  };
  const deleteItem = async () => {
    if (deleting) return;
    setDeleting(true);
    try {
      await workspace.deleteItem(item.id);
    } finally {
      setDeleting(false);
      setConfirmDelete(false);
    }
  };

  return (
    <div className="learning-detail-content">
      <header className="learning-detail-header">
        <div className="learning-detail-title-row">
          <div>
            <span>{t(`learning.kinds.${item.kind}`)} · {t(`learning.statuses.${item.status}`)}</span>
            <h2>{item.kind === "word" ? item.selected_text || primaryEntry?.term || item.working_text : t(`learning.detail.titles.${item.kind}`)}</h2>
            {item.kind === "word" && primaryEntry?.reading && <p className="learning-word-reading" lang={item.source_language ?? undefined}>{primaryEntry.reading}</p>}
          </div>
          <div className="learning-item-management" ref={managementRef} onKeyDown={(event) => {
            if (event.key === "Escape") managementButtonRef.current?.focus();
          }}>
            <button
              className="secondary-button"
              type="button"
              ref={managementButtonRef}
              aria-label={t("learning.actions.more")}
              data-tooltip={t("learning.actions.more")}
              aria-expanded={managementOpen}
              aria-controls={managementOpen ? `learning-item-actions-${item.id}` : undefined}
              onClick={() => setManagementOpen(!managementOpen)}
            ><MoreHorizontal size={18} /></button>
            {managementMenuMounted && (
              <div
                className="dropdown-menu learning-management-menu"
                id={`learning-item-actions-${item.id}`}
                ref={managementMenuRef}
                role="group"
                aria-label={t("learning.actions.more")}
                aria-hidden={!managementOpen}
                inert={!managementOpen}
              >
                <button className="dropdown-option" type="button" disabled={busy} onClick={() => {
                  setManagementOpen(false);
                  void (archived ? workspace.restoreItem(item.id) : workspace.archiveItem(item.id));
                }}>
                  {archived ? <RotateCcw size={14} /> : <Archive size={14} />}
                  {t(archived ? "learning.actions.restore" : "learning.actions.archive")}
                </button>
                <button className="dropdown-option learning-delete-action" type="button" disabled={busy} onClick={() => {
                  setManagementOpen(false);
                  setConfirmDelete(true);
                }}><Trash2 size={14} />{t("learning.actions.delete")}</button>
              </div>
            )}
          </div>
        </div>
        <div className="learning-detail-navigation">
          <button className="secondary-button learning-back-button" type="button" onClick={onBack}><ArrowLeft size={15} />{t("learning.inbox.title")}</button>
          <div className="learning-tabs" data-active-index={view === "card" ? 1 : 0} role="group" aria-label={t("learning.detail.views")}>
            <button type="button" className={view === "study" ? "active" : ""} aria-pressed={view === "study"} onClick={() => onViewChange("study")}><BookOpenText size={15} />{t("learning.detail.study")}</button>
            <button type="button" className={view === "card" ? "active" : ""} aria-pressed={view === "card"} onClick={() => onViewChange("card")}><CreditCard size={15} />{t("learning.detail.card")}</button>
          </div>
        </div>
      </header>

      <div className="learning-detail-scroll" ref={scrollRef} onScroll={onScroll} data-floating-boundary>
        {workspace.itemErrors[item.id] && <p className="learning-error" role="alert">{workspace.itemErrors[item.id]}</p>}

        <div className="learning-detail-view" hidden={view !== "study"}>
          <section className="learning-copy-section">
            {item.kind === "word" && primaryEntry && (
              <div className="learning-dictionary-meaning">
                <span>{t("learning.detail.dictionarySnapshot")}{primaryEntry.dictionary ? ` · ${primaryEntry.dictionary}` : ""}</span>
                <p>{primaryEntry.definition}</p>
              </div>
            )}
            <div className="learning-section-heading">
              <h3>{textLabel}</h3>
              {workingTextDirty && <span role="status">{t("learning.detail.unsaved")}</span>}
              {!editing && <button className="secondary-button" type="button" disabled={busy || archived} onClick={() => setEditing(true)}>{t("learning.detail.edit")}</button>}
            </div>
            {editing ? (
              <>
                <p className="learning-inline-hint">{t("learning.detail.correctionHint")}</p>
                <div className="learning-field">
                  <textarea aria-label={textLabel} value={workingText} disabled={busy || archived} onChange={(event) => workspace.editItem(item.id, { workingText: event.target.value })} />
                </div>
                <div className="learning-copy-actions">
                  <button className="secondary-button" type="button" disabled={busy} onClick={() => {
                    workspace.editItem(item.id, { workingText: undefined });
                    setEditing(false);
                  }}>{t("common.cancel")}</button>
                  <button className="primary-button" type="button" disabled={busy || archived || !workingText.trim()} onClick={() => void prepareWorkingText().then((saved) => saved && setEditing(false))}>
                    <Save size={15} />{t("learning.detail.saveWorkingText")}
                  </button>
                </div>
              </>
            ) : <p className="learning-reading-text" lang={item.source_language ?? undefined}>
              {item.kind === "word" && item.selected_text
                ? workingText.split(item.selected_text).map((part, index) => <span key={index}>{index > 0 && <mark>{item.selected_text}</mark>}{part}</span>)
                : workingText}
            </p>}
            {item.source_translation && <div className="learning-source-translation"><span>{t("learning.detail.sourceTranslation")}</span><p>{item.source_translation}</p></div>}
            {workingText !== item.source_text && <details className="learning-disclosure">
              <summary>{t("learning.detail.viewSource")}<ChevronDown size={15} aria-hidden="true" /></summary>
              <div className="learning-disclosure-body">
                <span className="learning-inline-hint">{t("learning.detail.sourceText")}</span>
                <p className="learning-reading-text" lang={item.source_language ?? undefined}>{item.source_text}</p>
              </div>
            </details>}
            {collapsedEntries.length > 0 && (
              <details className="learning-dictionary-snapshot">
                <summary>
                  <span>{t(item.kind === "word" ? "learning.detail.moreDefinitions" : "learning.detail.dictionarySnapshot")}</span>
                  <span className="learning-dictionary-count">{collapsedEntries.length}</span>
                  <ChevronDown size={15} aria-hidden="true" />
                </summary>
                <div className="learning-dictionary-entries">
                  {collapsedEntries.map((entry, index) => (
                    <p key={`${entry.dictionary ?? "local"}-${entry.term}-${index}`}><strong>{entry.term}{entry.reading ? ` · ${entry.reading}` : ""}{entry.dictionary ? ` · ${entry.dictionary}` : ""}</strong>{entry.definition}</p>
                  ))}
                </div>
              </details>
            )}
            <div className="learning-copy-actions">
              <button className="primary-button" type="button" aria-busy={generating} disabled={busy || (!draft && (archived || !workingText.trim()))} onClick={() => void openCard()}>
                {generating ? <RefreshCw className="learning-spin" size={15} /> : draft ? <Pencil size={15} /> : <CreditCard size={15} />}{t(generating ? "common.loading" : draft ? "learning.card.openDraft" : workingTextDirty ? "learning.card.saveAndCreate" : "learning.card.create")}
              </button>
            </div>
          </section>

          <section className="learning-ai-section">
            <div className="learning-section-heading learning-ai-heading"><h3><Sparkles size={16} aria-hidden="true" />{t("learning.ai.title")}</h3></div>
            <div className="learning-task-actions">
              <DropdownField
                compact
                floating
                label={t("learning.ai.mode")}
                value={task}
                options={(["contextual_word_explanation", "sentence_analysis", "session_review"] as const).map((value) => ({ value, label: t(`learning.ai.tasks.${value}`) }))}
                disabled={busy || archived}
                onChange={(value) => setTask(value as LearningTaskType)}
              />
              <button className="primary-button" type="button" aria-busy={analyzing} disabled={analysisDisabled} onClick={() => void analyze(task)}>
                {analyzing && <RefreshCw className="learning-spin" size={15} />}
                {t(analyzing ? "learning.ai.analyzing" : item.analysis?.task_type === task ? "learning.ai.reanalyze" : "learning.ai.analyze")}
              </button>
            </div>
            {!canUseAi && <div className="learning-ai-setup">
              <p className="learning-inline-hint">{t("learning.ai.configureHint")}</p>
              <button className="secondary-button" type="button" onClick={onOpenAiSettings}>{t("learning.ai.openSettings")}</button>
            </div>}
            {analyzing && <p className="learning-inline-hint" role="status">{t("learning.ai.analyzing")}</p>}
            {item.analysis && (
              <>
                <div className="learning-focus-actions" role="group" aria-label={t("learning.ai.quickFocus")}>
                  <span>{t("learning.ai.quickFocus")}</span>
                  {(["simpler", "examples", "compare"] as const).map((focus) => (
                    <button key={focus} type="button" disabled={analysisDisabled} onClick={() => void analyze(item.analysis!.task_type, focus)}>{t(`learning.ai.focus.${focus}`)}</button>
                  ))}
                </div>
                {(item.analysis_stale || workingTextDirty) && <p className="learning-inline-hint" role="status">{t("learning.analysis.stale")}</p>}
                <LearningAnalysisView analysis={item.analysis} showHeading={false} />
              </>
            )}
          </section>
        </div>

        <div className="learning-detail-view" hidden={view !== "card"}>
          <LearningCardEditor item={item} workspace={workspace} ankiEnabled={ankiEnabled} ankiAvailable={ankiAvailable} workingText={workingText} prepareWorkingText={prepareWorkingText} />
        </div>
      </div>
      {confirmDelete && (
        <ConfirmDialog title={t("learning.actions.delete")} message={t("learning.actions.deleteConfirm")} confirmLabel={t("learning.actions.delete")} busy={deleting} icon={<Trash2 size={16} />} onClose={() => setConfirmDelete(false)} onConfirm={deleteItem} />
      )}
    </div>
  );
}
