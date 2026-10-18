import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Archive,
  BookOpenText,
  ChevronDown,
  RefreshCw,
  RotateCcw,
  Save,
  Search,
  Sparkles,
  Trash2,
} from "lucide-react";

import { timestamp } from "../../app/app-utils";
import { ConfirmDialog } from "../../app/components/ConfirmDialog";
import type { LearningWorkspaceController } from "../hooks/useLearningWorkspace";
import { learningTaskForKind } from "../learning";
import type {
  LearningItem,
  LearningItemKind,
  LearningTaskType,
} from "../types";
import { DropdownField } from "../../shared/ui/DropdownField";
import { LearningAnalysisView } from "./LearningAnalysisView";
import { LearningCardEditor } from "./LearningCardEditor";

export function LearningInbox({
  workspace,
  ankiEnabled,
  ankiAvailable,
}: {
  workspace: LearningWorkspaceController;
  ankiEnabled: boolean;
  ankiAvailable: boolean;
}) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [kindFilter, setKindFilter] = useState<LearningItemKind | "all">("all");
  const filteredItems = useMemo(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase();
    return workspace.items.filter((candidate) => {
      if (kindFilter !== "all" && candidate.kind !== kindFilter) return false;
      if (normalizedQuery) {
        const searchable = [
          candidate.selected_text,
          candidate.working_text,
          candidate.source_text,
          candidate.source_translation,
          candidate.analysis?.summary,
          candidate.analysis?.current_meaning,
        ]
          .filter(Boolean)
          .join("\n")
          .toLocaleLowerCase();
        if (!searchable.includes(normalizedQuery)) return false;
      }
      return true;
    });
  }, [kindFilter, query, workspace.items]);
  const item = filteredItems.find((candidate) => candidate.id === workspace.selectedId)
    ?? filteredItems[0]
    ?? null;
  const filtersActive = query.trim() !== "" || kindFilter !== "all";
  const selectionHidden = workspace.selectedId !== null
    && !filteredItems.some((candidate) => candidate.id === workspace.selectedId);

  // 从外部打开（例如字幕里的「打开学习条目」）被本地筛选隐藏的条目时，清空筛选让它可见。
  useEffect(() => {
    if (!selectionHidden || !workspace.items.some((candidate) => candidate.id === workspace.selectedId)) return;
    setQuery("");
    setKindFilter("all");
  }, [workspace.selectedId]);

  // 用户修改筛选后，把选中状态同步到详情区实际显示的条目。
  useEffect(() => {
    if (selectionHidden && filteredItems[0]) workspace.setSelectedId(filteredItems[0].id);
  }, [query, kindFilter]);

  return (
    <div className="learning-inbox">
      <aside className="learning-item-pane">
        <div className="learning-pane-toolbar">
          <div className="learning-pane-toolbar-top">
            <div>
              <h2>{t("learning.inbox.title")}</h2>
              <span>{t("learning.inbox.count", { count: filteredItems.length })}</span>
            </div>
            <DropdownField
              compact
              label={t("learning.inbox.statusFilter")}
              value={workspace.statusFilter}
              options={statusOptions(t)}
              onChange={(value) => workspace.setStatusFilter(value as typeof workspace.statusFilter)}
            />
          </div>
          <div className="learning-pane-filters">
            <div className="learning-material-search">
              <Search size={15} />
              <input
                value={query}
                type="search"
                placeholder={t("learning.materials.searchPlaceholder")}
                aria-label={t("learning.materials.search")}
                onChange={(event) => setQuery(event.target.value)}
              />
            </div>
            <DropdownField
              compact
              label={t("learning.inbox.kindFilter")}
              value={kindFilter}
              options={kindOptions(t)}
              onChange={(value) => setKindFilter(value as LearningItemKind | "all")}
            />
          </div>
        </div>
        {workspace.error && <p className="learning-error" role="alert">{workspace.error}</p>}
        {workspace.loading ? (
          <div className="learning-empty"><RefreshCw className="learning-spin" size={20} /><p>{t("common.loading")}</p></div>
        ) : filteredItems.length ? (
          <div className="learning-item-list">
            {filteredItems.map((candidate) => (
              <LearningItemRow
                key={candidate.id}
                item={candidate}
                active={candidate.id === item?.id}
                locale={workspace.locale}
                onSelect={() => workspace.setSelectedId(candidate.id)}
              />
            ))}
          </div>
        ) : (
          <div className="learning-empty"><BookOpenText size={22} /><p>{t("learning.inbox.empty")}</p></div>
        )}
        {workspace.hasMore && filtersActive && <p className="learning-inline-hint">{t("learning.inbox.searchLoadedOnly")}</p>}
        {workspace.hasMore && (
          <div className="learning-load-more">
            <button className="secondary-button" type="button" disabled={workspace.loadingMore} onClick={() => void workspace.loadMore()}>
              {t(workspace.loadingMore ? "learning.inbox.loadingMore" : "learning.inbox.loadMore")}
            </button>
          </div>
        )}
      </aside>
      <main className="learning-detail-pane">
        {item ? (
          <LearningItemDetail item={item} workspace={workspace} ankiEnabled={ankiEnabled} ankiAvailable={ankiAvailable} />
        ) : (
          <div className="learning-empty"><BookOpenText size={22} /><p>{t("learning.inbox.selectHint")}</p></div>
        )}
      </main>
    </div>
  );
}

function LearningItemRow({
  item,
  active,
  locale,
  onSelect,
}: {
  item: LearningItem;
  active: boolean;
  locale: string;
  onSelect: () => void;
}) {
  const { t } = useTranslation();
  return (
    <button className={`learning-item-row ${active ? "active" : ""}`} type="button" onClick={onSelect}>
      <span className="learning-item-row-meta">
        <span>{t(`learning.kinds.${item.kind}`)}</span>
        <time>{timestamp(item.updated_at, locale)}</time>
      </span>
      <strong>{item.selected_text || firstLine(item.working_text)}</strong>
      <p>{item.working_text}</p>
      <span className={`learning-status learning-status-${item.status}`}>{t(`learning.statuses.${item.status}`)}</span>
    </button>
  );
}

function LearningItemDetail({
  item,
  workspace,
  ankiEnabled,
  ankiAvailable,
}: {
  item: LearningItem;
  workspace: LearningWorkspaceController;
  ankiEnabled: boolean;
  ankiAvailable: boolean;
}) {
  const { t } = useTranslation();
  const [workingText, setWorkingText] = useState(item.working_text);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const busy = workspace.isItemBusy(item.id);
  const archived = item.status === "archived";
  const task = learningTaskForKind(item.kind);
  const canUseAi = Boolean(workspace.preferences.profileId && workspace.preferences.model.trim());
  const itemError = workspace.itemErrors[item.id];

  useEffect(() => setWorkingText(item.working_text), [item.id, item.working_text]);

  const prepareWorkingText = async (): Promise<boolean> => {
    if (archived) return false;
    if (workingText === item.working_text) return true;
    return Boolean(await workspace.updateWorkingText(item.id, workingText));
  };
  const analyze = async (taskType: LearningTaskType, focus?: "simpler" | "examples" | "compare") => {
    if (!workingText.trim() || !await prepareWorkingText()) return;
    await workspace.analyze(item.id, taskType, focus);
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
        <div>
          <span>{t(`learning.kinds.${item.kind}`)} · {t(`learning.statuses.${item.status}`)}</span>
          <h2>{item.selected_text || firstLine(item.working_text)}</h2>
        </div>
        <div className="learning-item-management">
          {item.status === "archived" ? (
            <button className="secondary-button" type="button" disabled={busy} onClick={() => void workspace.restoreItem(item.id)}><RotateCcw size={14} />{t("learning.actions.restore")}</button>
          ) : (
            <button className="secondary-button" type="button" disabled={busy} onClick={() => void workspace.archiveItem(item.id)}><Archive size={14} />{t("learning.actions.archive")}</button>
          )}
          <button
            className="learning-danger-button"
            type="button"
            disabled={busy}
            onClick={() => setConfirmDelete(true)}
          ><Trash2 size={14} />{t("learning.actions.delete")}</button>
        </div>
      </header>

      {itemError && <p className="learning-error" role="alert">{itemError}</p>}

      <section className="learning-copy-section">
        <label className="learning-field learning-field-wide learning-source-field">
          <span>{t("learning.detail.sourceText")}</span>
          <textarea value={item.source_text} readOnly />
        </label>
        <label className="learning-field learning-field-wide">
          <span>{t("learning.detail.workingText")}</span>
          <textarea value={workingText} disabled={busy || archived} onChange={(event) => setWorkingText(event.target.value)} />
        </label>
        <div className="learning-copy-actions">
          <button className="secondary-button" type="button" disabled={busy || archived || workingText === item.working_text || !workingText.trim()} onClick={() => void prepareWorkingText()}>
            <Save size={15} />{t("learning.detail.saveWorkingText")}
          </button>
        </div>
        {item.source_translation && <div className="learning-source-translation"><span>{t("learning.detail.sourceTranslation")}</span><p>{item.source_translation}</p></div>}
        {item.dictionary_entries.length > 0 && (
          <details className="learning-dictionary-snapshot">
            <summary>
              <span>{t("learning.detail.dictionarySnapshot")}</span>
              <span className="learning-dictionary-count">{item.dictionary_entries.length}</span>
              <ChevronDown size={15} aria-hidden="true" />
            </summary>
            <div className="learning-dictionary-entries">
              {item.dictionary_entries.map((entry, index) => (
                <p key={`${entry.dictionary ?? "local"}-${entry.term}-${index}`}><strong>{entry.term}{entry.reading ? ` · ${entry.reading}` : ""}</strong>{entry.definition}</p>
              ))}
            </div>
          </details>
        )}
      </section>

      <section className="learning-ai-section">
        <div className="learning-section-heading">
          <div><h3>{t("learning.ai.title")}</h3><p>{t("learning.ai.description")}</p></div>
          <Sparkles size={18} />
        </div>
        {!canUseAi && <p className="learning-inline-hint">{t("learning.ai.configureHint")}</p>}
        <div className="learning-task-actions">
          <TaskButton task="contextual_word_explanation" onAnalyze={analyze} disabled={busy || archived || !canUseAi || !workingText.trim()} />
          <TaskButton task="sentence_analysis" onAnalyze={analyze} disabled={busy || archived || !canUseAi || !workingText.trim()} />
          <TaskButton task="session_review" onAnalyze={analyze} disabled={busy || archived || !canUseAi || !workingText.trim()} />
        </div>
        <div className="learning-focus-actions">
          <span>{t("learning.ai.quickFocus")}</span>
          <button type="button" disabled={busy || archived || !canUseAi || !workingText.trim()} onClick={() => void analyze(task, "simpler")}>{t("learning.ai.focus.simpler")}</button>
          <button type="button" disabled={busy || archived || !canUseAi || !workingText.trim()} onClick={() => void analyze(task, "examples")}>{t("learning.ai.focus.examples")}</button>
          <button type="button" disabled={busy || archived || !canUseAi || !workingText.trim()} onClick={() => void analyze(task, "compare")}>{t("learning.ai.focus.compare")}</button>
        </div>
      </section>

      {item.analysis && <LearningAnalysisView analysis={item.analysis} />}
      <LearningCardEditor
        item={item}
        workspace={workspace}
        ankiEnabled={ankiEnabled} ankiAvailable={ankiAvailable}
        workingText={workingText}
        prepareWorkingText={prepareWorkingText}
      />
      {confirmDelete && (
        <ConfirmDialog
          title={t("learning.actions.delete")}
          message={t("learning.actions.deleteConfirm")}
          confirmLabel={t("learning.actions.delete")}
          busy={deleting}
          icon={<Trash2 size={16} />}
          onClose={() => setConfirmDelete(false)}
          onConfirm={deleteItem}
        />
      )}
    </div>
  );
}

function TaskButton({
  task,
  onAnalyze,
  disabled,
}: {
  task: LearningTaskType;
  onAnalyze: (task: LearningTaskType) => Promise<void>;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  return <button className="secondary-button" type="button" disabled={disabled} onClick={() => void onAnalyze(task)}><Sparkles size={14} />{t(`learning.ai.tasks.${task}`)}</button>;
}

function firstLine(value: string): string {
  return value.split(/\r?\n/, 1)[0]?.trim() || "—";
}

function statusOptions(t: (key: string) => string) {
  return [
    { value: "all", label: t("learning.filters.all") },
    { value: "collected", label: t("learning.filters.collected") },
    { value: "analyzed", label: t("learning.filters.analyzed") },
    { value: "card_draft", label: t("learning.filters.cardDraft") },
    { value: "exported", label: t("learning.filters.exported") },
    { value: "archived", label: t("learning.filters.archived") },
  ];
}

function kindOptions(t: (key: string) => string) {
  return [
    { value: "all", label: t("learning.filters.all") },
    { value: "word", label: t("learning.kinds.word") },
    { value: "sentence", label: t("learning.kinds.sentence") },
    { value: "excerpt", label: t("learning.kinds.excerpt") },
  ];
}
