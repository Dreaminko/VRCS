import { invoke, isTauri } from "@tauri-apps/api/core";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import {
  ArrowDown,
  ArrowUp,
  Download,
  Link2,
  Pencil,
  Plus,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { coreApi } from "../../api";
import { localizedError } from "../../app/app-utils";
import type {
  GlossaryLocalSource,
  GlossarySource,
  GlossarySourceStatus,
  GlossarySubscriptionSource,
} from "../../types";
import { LocalGlossaryDialog, SubscriptionGlossaryDialog } from "./GlossaryDialogs";
import {
  emptyGlossaryEntry,
  exportFileName,
  glossarySourceId,
  validateLocalDraft,
  validateSubscriptionDraft,
  type LocalGlossaryDraft,
  type SubscriptionGlossaryDraft,
} from "./glossary-utils";

type Translate = ReturnType<typeof useTranslation>["t"];

function statusLabel(status: GlossarySourceStatus | undefined, enabled: boolean, loaded: boolean, t: Translate): string {
  if (!enabled) return t("settings.glossary.glossaryStates.disabled");
  if (!loaded) return t("settings.glossary.glossaryStates.loading");
  if (!status) return t("settings.glossary.glossaryStates.unavailable");
  const knownStates: Record<string, string> = {
    idle: t("settings.glossary.glossaryStates.idle"),
    refreshing: t("settings.glossary.glossaryStates.refreshing"),
    ready: t("settings.glossary.glossaryStates.ready"),
    stale: t("settings.glossary.glossaryStates.stale"),
    error: t("settings.glossary.glossaryStates.error"),
  };
  return knownStates[status.state] ?? status.state;
}

export function GlossaryEditor({
  sources,
  disabled,
  onChange,
}: {
  sources: GlossarySource[];
  disabled: boolean;
  onChange: (
    sources: GlossarySource[],
    afterSave?: () => void,
    afterError?: () => void,
  ) => void;
}) {
  const { t } = useTranslation();
  const [statuses, setStatuses] = useState<GlossarySourceStatus[]>([]);
  const [statusesLoaded, setStatusesLoaded] = useState(false);
  const [statusError, setStatusError] = useState("");
  const [refreshingId, setRefreshingId] = useState<string | null>(null);
  const [localDraft, setLocalDraft] = useState<LocalGlossaryDraft | null>(null);
  const [subscriptionDraft, setSubscriptionDraft] = useState<SubscriptionGlossaryDraft | null>(null);
  const [editorSaving, setEditorSaving] = useState(false);
  const [editorError, setEditorError] = useState("");
  const returnFocusRef = useRef<HTMLButtonElement>(null);
  const statusById = useMemo(
    () => new Map(statuses.map((status) => [status.id, status])),
    [statuses],
  );

  const loadStatuses = useCallback(async () => {
    try {
      setStatuses(await coreApi.glossaryStatuses());
      setStatusesLoaded(true);
      setStatusError("");
    } catch (reason) {
      setStatusesLoaded(true);
      setStatusError(localizedError(reason, t, "errors.translation.failed"));
    }
  }, [t]);

  useEffect(() => {
    void loadStatuses();
  }, [loadStatuses]);

  useEffect(() => {
    if (!statuses.some((status) => status.state === "idle" || status.state === "refreshing")) return;
    const timeout = window.setTimeout(() => void loadStatuses(), 1000);
    return () => window.clearTimeout(timeout);
  }, [loadStatuses, statuses]);

  const persistSources = (
    next: GlossarySource[],
    afterSave?: () => void,
    afterError?: () => void,
  ) => {
    onChange(
      next,
      () => {
        void loadStatuses();
        afterSave?.();
      },
      afterError,
    );
  };

  const openLocalEditor = (trigger: HTMLButtonElement, source?: GlossaryLocalSource) => {
    returnFocusRef.current = trigger;
    setEditorError("");
    setLocalDraft(source ? {
      id: source.id,
      name: source.name,
      entries: source.entries.map((entry) => ({ ...entry })),
    } : {
      id: null,
      name: "",
      entries: [emptyGlossaryEntry()],
    });
  };

  const openSubscriptionEditor = (trigger: HTMLButtonElement, source?: GlossarySubscriptionSource) => {
    returnFocusRef.current = trigger;
    setEditorError("");
    setSubscriptionDraft(source ? {
      id: source.id,
      url: source.url,
      displayName: source.display_name ?? "",
    } : {
      id: null,
      url: "",
      displayName: "",
    });
  };

  const saveLocalDraft = () => {
    if (!localDraft) return;
    const validationError = validateLocalDraft(localDraft, t);
    if (validationError) {
      setEditorError(validationError);
      return;
    }
    setEditorSaving(true);
    setEditorError("");
    const normalized: GlossaryLocalSource = {
      id: localDraft.id ?? glossarySourceId("local"),
      type: "local",
      name: localDraft.name.trim(),
      enabled: localDraft.id
        ? (sources.find((source) => source.id === localDraft.id)?.enabled ?? true)
        : true,
      entries: localDraft.entries.map((entry) => ({
        ...entry,
        source: entry.source.trim(),
      })),
    };
    const next = localDraft.id
      ? sources.map((source) => source.id === localDraft.id ? normalized : source)
      : [normalized, ...sources];
    persistSources(next, () => {
      setEditorSaving(false);
      setLocalDraft(null);
    }, () => {
      setEditorSaving(false);
      setEditorError(t("settings.glossary.glossarySaveFailed"));
    });
  };

  const saveSubscriptionDraft = () => {
    if (!subscriptionDraft) return;
    const validationError = validateSubscriptionDraft(subscriptionDraft, t);
    if (validationError) {
      setEditorError(validationError);
      return;
    }
    setEditorSaving(true);
    setEditorError("");
    const normalized: GlossarySubscriptionSource = {
      id: subscriptionDraft.id ?? glossarySourceId("subscription"),
      type: "subscription",
      url: subscriptionDraft.url.trim(),
      display_name: subscriptionDraft.displayName.trim() || null,
      enabled: subscriptionDraft.id
        ? (sources.find((source) => source.id === subscriptionDraft.id)?.enabled ?? true)
        : true,
    };
    const next = subscriptionDraft.id
      ? sources.map((source) => source.id === subscriptionDraft.id ? normalized : source)
      : [...sources, normalized];
    persistSources(next, () => {
      setEditorSaving(false);
      setSubscriptionDraft(null);
    }, () => {
      setEditorSaving(false);
      setEditorError(t("settings.glossary.glossarySaveFailed"));
    });
  };

  const moveSource = (index: number, direction: -1 | 1) => {
    const targetIndex = index + direction;
    if (targetIndex < 0 || targetIndex >= sources.length) return;
    const next = [...sources];
    [next[index], next[targetIndex]] = [next[targetIndex], next[index]];
    persistSources(next);
  };

  const removeSource = (source: GlossarySource) => {
    const status = statusById.get(source.id);
    const name = source.type === "local"
      ? source.name
      : source.display_name || status?.name || source.url;
    if (!window.confirm(t("settings.glossary.confirmDeleteGlossary", { name }))) return;
    persistSources(sources.filter((item) => item.id !== source.id));
  };

  const refreshSource = async (id: string) => {
    setRefreshingId(id);
    setStatusError("");
    try {
      await coreApi.refreshGlossary(id);
      await loadStatuses();
    } catch (reason) {
      setStatusError(localizedError(reason, t, "errors.translation.failed"));
      await loadStatuses();
    } finally {
      setRefreshingId(null);
    }
  };

  const exportSource = async (source: GlossaryLocalSource) => {
    const payload = `${JSON.stringify({
      version: 1,
      name: source.name,
      entries: source.entries,
    }, null, 2)}\n`;
    try {
      if (isTauri()) {
        const path = await saveDialog({
          defaultPath: exportFileName(source.name),
          filters: [{ name: "JSON", extensions: ["json"] }],
        });
        if (!path) return;
        await invoke("write_glossary_file", { path, contents: payload });
        return;
      }
      const url = URL.createObjectURL(new Blob([payload], { type: "application/json;charset=utf-8" }));
      const link = document.createElement("a");
      link.href = url;
      link.download = exportFileName(source.name);
      document.body.appendChild(link);
      link.click();
      link.remove();
      URL.revokeObjectURL(url);
    } catch (reason) {
      setStatusError(localizedError(reason, t, "settings.glossary.glossaryExportFailed"));
    }
  };

  return (
    <div className="glossary-manager">
      <div className="glossary-heading">
        <span>
          <strong>{t("settings.glossary.sourcesTitle")}</strong>
          <small>{t("settings.glossary.glossaryPriorityHint")}</small>
        </span>
        <div className="glossary-heading-actions">
          <button
            className="secondary-button"
            type="button"
            disabled={disabled}
            onClick={(event) => openLocalEditor(event.currentTarget)}
          >
            <Plus size={14} aria-hidden="true" />
            {t("settings.glossary.addGlossary")}
          </button>
          <button
            className="secondary-button"
            type="button"
            disabled={disabled}
            onClick={(event) => openSubscriptionEditor(event.currentTarget)}
          >
            <Link2 size={14} aria-hidden="true" />
            {t("settings.glossary.addSubscription")}
          </button>
        </div>
      </div>

      {statusError && <small className="api-model-catalog-error" aria-live="polite">{statusError}</small>}
      <div className="glossary-list">
        {sources.length === 0 && (
          <div className="glossary-empty">
            <strong>{t("settings.glossary.noGlossaries")}</strong>
            <small>{t("settings.glossary.noGlossariesHint")}</small>
          </div>
        )}
        {sources.map((source, index) => {
          const status = statusById.get(source.id);
          const name = source.type === "local"
            ? source.name
            : source.display_name || status?.name || source.url;
          const entryCount = status?.entry_count ?? (source.type === "local" ? source.entries.length : 0);
          const state = statusLabel(status, source.enabled, statusesLoaded, t);
          const statusClass = !source.enabled
            ? "disabled"
            : status?.state === "error" || status?.state === "stale"
              ? "error"
              : status?.state === "ready"
                ? "ready"
                : "pending";
          return (
            <article className={`glossary-card ${source.enabled ? "enabled" : ""}`} key={source.id}>
              <div className="glossary-card-main">
                <div className="glossary-card-title">
                  <span className={`glossary-type ${source.type}`}>
                    {t(`settings.glossary.glossaryTypes.${source.type}`)}
                  </span>
                  <strong>{name}</strong>
                </div>
                <small className="glossary-card-detail">
                  {source.type === "subscription" ? source.url : t("settings.glossary.localGlossaryDetail")}
                </small>
                <div className="glossary-card-meta">
                  <span>{t("settings.glossary.glossaryEntryCount", { count: entryCount })}</span>
                  <span className={`glossary-status ${statusClass}`}>{state}</span>
                  {status?.omitted_entry_count ? (
                    <span>{t("settings.glossary.glossaryEntriesOmitted", { count: status.omitted_entry_count })}</span>
                  ) : null}
                </div>
                {status?.detail && (status.state === "error" || status.state === "stale") && (
                  <small className="glossary-status-detail">{status.detail}</small>
                )}
              </div>
              <div className="glossary-card-controls">
                <button
                  className="settings-switch-button"
                  type="button"
                  role="switch"
                  aria-checked={source.enabled}
                  aria-label={t("settings.glossary.enableGlossary", { name })}
                  disabled={disabled}
                  onClick={() => persistSources(sources.map((item) => item.id === source.id
                    ? { ...item, enabled: !item.enabled }
                    : item))}
                >
                  <span className="switch-track" aria-hidden="true"><span /></span>
                </button>
                <div className="glossary-order-actions">
                  <button
                    className="api-row-icon-button"
                    type="button"
                    aria-label={t("settings.glossary.moveGlossaryUp", { name })}
                    title={t("settings.glossary.moveUp")}
                    disabled={disabled || index === 0}
                    onClick={() => moveSource(index, -1)}
                  >
                    <ArrowUp size={15} aria-hidden="true" />
                  </button>
                  <button
                    className="api-row-icon-button"
                    type="button"
                    aria-label={t("settings.glossary.moveGlossaryDown", { name })}
                    title={t("settings.glossary.moveDown")}
                    disabled={disabled || index === sources.length - 1}
                    onClick={() => moveSource(index, 1)}
                  >
                    <ArrowDown size={15} aria-hidden="true" />
                  </button>
                </div>
              </div>
              <div className="glossary-card-actions">
                {source.type === "local" ? (
                  <>
                    <button
                      className="secondary-button"
                      type="button"
                      disabled={disabled}
                      onClick={(event) => openLocalEditor(event.currentTarget, source)}
                    >
                      <Pencil size={14} aria-hidden="true" />
                      {t("common.edit")}
                    </button>
                    <button
                      className="secondary-button"
                      type="button"
                      disabled={disabled}
                      onClick={() => void exportSource(source)}
                    >
                      <Download size={14} aria-hidden="true" />
                      {t("settings.glossary.exportGlossary")}
                    </button>
                  </>
                ) : (
                  <>
                    <button
                      className="secondary-button"
                      type="button"
                      disabled={disabled || refreshingId === source.id}
                      onClick={() => void refreshSource(source.id)}
                    >
                      <RefreshCw className={refreshingId === source.id ? "spin" : ""} size={14} aria-hidden="true" />
                      {t("common.refresh")}
                    </button>
                    <button
                      className="secondary-button"
                      type="button"
                      disabled={disabled}
                      onClick={(event) => openSubscriptionEditor(event.currentTarget, source)}
                    >
                      <Pencil size={14} aria-hidden="true" />
                      {t("common.edit")}
                    </button>
                  </>
                )}
                <button
                  className="secondary-button api-danger-button"
                  type="button"
                  disabled={disabled}
                  onClick={() => removeSource(source)}
                >
                  <Trash2 size={14} aria-hidden="true" />
                  {t("common.delete")}
                </button>
              </div>
            </article>
          );
        })}
      </div>

      {localDraft && (
        <LocalGlossaryDialog
          draft={localDraft}
          saving={editorSaving}
          error={editorError}
          returnFocusRef={returnFocusRef}
          onChange={(draft) => {
            setEditorError("");
            setLocalDraft(draft);
          }}
          onError={setEditorError}
          onSave={saveLocalDraft}
          onClose={() => {
            setEditorError("");
            setLocalDraft(null);
          }}
        />
      )}
      {subscriptionDraft && (
        <SubscriptionGlossaryDialog
          draft={subscriptionDraft}
          saving={editorSaving}
          error={editorError}
          returnFocusRef={returnFocusRef}
          onChange={(draft) => {
            setEditorError("");
            setSubscriptionDraft(draft);
          }}
          onSave={saveSubscriptionDraft}
          onClose={() => {
            setEditorError("");
            setSubscriptionDraft(null);
          }}
        />
      )}
    </div>
  );
}
