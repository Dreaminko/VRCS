import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { BookOpenText, RefreshCw, Search, X } from "lucide-react";

import { timestamp } from "../../app/app-utils";
import { INTERFACE_LAYOUT_CHANGE_EVENT, interfaceLayoutPixels } from "../../app/interface-scale";
import { DropdownField } from "../../shared/ui/DropdownField";
import type { LearningWorkspaceController } from "../hooks/useLearningWorkspace";
import type { LearningItem, LearningItemKind } from "../types";
import { LearningItemDetail } from "./LearningItemDetail";

export function LearningInbox({
  workspace,
  ankiEnabled,
  ankiAvailable,
  onOpenAiSettings,
  detailOpen,
  onDetailOpen,
  visible,
  onOpenMaterials,
}: {
  workspace: LearningWorkspaceController;
  ankiEnabled: boolean;
  ankiAvailable: boolean;
  onOpenAiSettings: () => void;
  detailOpen: boolean;
  onDetailOpen: (open: boolean) => void;
  visible: boolean;
  onOpenMaterials: () => void;
}) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [kindFilter, setKindFilter] = useState<LearningItemKind | "all">("all");
  const [view, setView] = useState<"study" | "card">("study");
  const [tooltip, setTooltip] = useState<{ label: string; top: number; left: number; keyboard: boolean } | null>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);
  const detailPaneRef = useRef<HTMLElement>(null);
  const detailScrollRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const scrollPositions = useRef<Record<string, number>>({});
  const hideTooltip = () => setTooltip(null);
  const showTooltip = (target: HTMLButtonElement, label: string, keyboard = false) => {
    const rect = target.getBoundingClientRect();
    setTooltip({
      label,
      top: interfaceLayoutPixels(rect.bottom) + 13,
      left: interfaceLayoutPixels(rect.left + rect.width / 2),
      keyboard,
    });
  };
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
        ].filter(Boolean).join("\n").toLocaleLowerCase();
        if (!searchable.includes(normalizedQuery)) return false;
      }
      return true;
    });
  }, [kindFilter, query, workspace.items]);
  const item = filteredItems.find((candidate) => candidate.id === workspace.selectedId)
    ?? filteredItems[0]
    ?? null;
  const selectionHidden = workspace.selectedId !== null
    && !filteredItems.some((candidate) => candidate.id === workspace.selectedId);
  const scrollKey = `${item?.id}:${view}`;

  // Reveal items opened from another page when local filters hide them.
  useEffect(() => {
    if (!selectionHidden || !workspace.items.some((candidate) => candidate.id === workspace.selectedId)) return;
    setQuery("");
    setKindFilter("all");
  }, [workspace.selectedId]);

  // Keep the controller selection aligned with locally filtered results.
  useEffect(() => {
    if (selectionHidden && filteredItems[0]) workspace.setSelectedId(filteredItems[0].id);
  }, [query, kindFilter]);

  useLayoutEffect(() => {
    if (visible && detailScrollRef.current) {
      detailScrollRef.current.scrollTop = scrollPositions.current[scrollKey] ?? 0;
    }
  }, [scrollKey, visible, detailOpen]);

  useLayoutEffect(() => {
    const element = tooltipRef.current;
    if (!tooltip || !element) return;
    const width = element.offsetWidth;
    const left = Math.max(12, Math.min(tooltip.left - width / 2, interfaceLayoutPixels(window.innerWidth) - width - 12));
    element.style.left = `${left}px`;
    element.style.setProperty("--tooltip-arrow-left", `${tooltip.left - left}px`);
  }, [tooltip]);

  useEffect(() => {
    if (!visible) hideTooltip();
  }, [visible]);

  useEffect(() => {
    document.addEventListener("scroll", hideTooltip, true);
    window.addEventListener("resize", hideTooltip);
    window.addEventListener(INTERFACE_LAYOUT_CHANGE_EVENT, hideTooltip);
    return () => {
      document.removeEventListener("scroll", hideTooltip, true);
      window.removeEventListener("resize", hideTooltip);
      window.removeEventListener(INTERFACE_LAYOUT_CHANGE_EVENT, hideTooltip);
    };
  }, []);

  const returnToList = () => {
    onDetailOpen(false);
    requestAnimationFrame(() => {
      listRef.current?.querySelector<HTMLButtonElement>('[aria-current="true"]')?.focus({ preventScroll: true });
    });
  };

  return (
    <div className="learning-inbox" data-detail-open={detailOpen && Boolean(item)}>
      <aside className="learning-item-pane" aria-label={t("learning.inbox.title")}>
        <div className="learning-pane-toolbar">
          <div className="learning-pane-toolbar-top learning-pane-header">
            <div className="learning-pane-title">
              <h2>{t("learning.inbox.title")}</h2>
              <span className="learning-inbox-count">{t("learning.inbox.count", { count: filteredItems.length })}</span>
            </div>
            <button
              className="dock-button learning-material-entry"
              type="button"
              aria-label={t("learning.materials.title")}
              onPointerEnter={(event) => {
                if (event.pointerType !== "touch") showTooltip(event.currentTarget, t("learning.materials.title"));
              }}
              onPointerLeave={hideTooltip}
              onFocus={(event) => {
                if (event.currentTarget.matches(":focus-visible")) {
                  showTooltip(event.currentTarget, t("learning.materials.title"), true);
                }
              }}
              onBlur={hideTooltip}
              onKeyDown={(event) => {
                if (event.key === "Escape") hideTooltip();
              }}
              onClick={() => {
                hideTooltip();
                onOpenMaterials();
              }}
            >
              <BookOpenText size={17} />
            </button>
          </div>
          <div className="learning-pane-filters">
            <div className="learning-material-search">
              <Search size={15} />
              <input
                value={query}
                type="search"
                placeholder={t("learning.materials.searchPlaceholder")}
                aria-label={t("learning.inbox.search")}
                onChange={(event) => setQuery(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key !== "Escape" || !query) return;
                  event.preventDefault();
                  setQuery("");
                }}
              />
              {query && (
                <button
                  className="learning-search-clear"
                  type="button"
                  aria-label={t("conversations.clearSearch")}
                  onClick={() => setQuery("")}
                >
                  <X size={14} />
                </button>
              )}
            </div>
            <div className="learning-pane-filter-options">
              <div className="learning-filter-field">
                <span>{t("learning.inbox.statusFilter")}</span>
                <DropdownField
                  compact
                  floating
                  label={t("learning.inbox.statusFilter")}
                  value={workspace.statusFilter}
                  options={statusOptions(t)}
                  onChange={(value) => workspace.setStatusFilter(value as typeof workspace.statusFilter)}
                />
              </div>
              <div className="learning-filter-field">
                <span>{t("learning.inbox.kindFilter")}</span>
                <DropdownField
                  compact
                  floating
                  label={t("learning.inbox.kindFilter")}
                  value={kindFilter}
                  options={kindOptions(t)}
                  onChange={(value) => setKindFilter(value as LearningItemKind | "all")}
                />
              </div>
            </div>
          </div>
          {workspace.hasMore && <p className="learning-inline-hint">{t("learning.inbox.searchLoadedOnly")}</p>}
        </div>
        {workspace.error && <p className="learning-error" role="alert">{workspace.error}</p>}
        <div className="learning-item-list" ref={listRef}>
          {workspace.loading ? (
            <div className="learning-empty" role="status"><RefreshCw className="learning-spin" size={20} /><p>{t("common.loading")}</p></div>
          ) : filteredItems.length ? (
            filteredItems.map((candidate) => (
              <LearningItemRow
                key={candidate.id}
                item={candidate}
                active={candidate.id === item?.id}
                locale={workspace.locale}
                onSelect={() => {
                  workspace.setSelectedId(candidate.id);
                  onDetailOpen(true);
                  requestAnimationFrame(() => {
                    if (!listRef.current?.offsetParent) detailPaneRef.current?.focus({ preventScroll: true });
                  });
                }}
              />
            ))
          ) : (
            <div className="learning-empty"><BookOpenText size={22} /><p>{t("learning.inbox.empty")}</p></div>
          )}
          {!workspace.loading && workspace.hasMore && (
            <footer className="learning-inbox-pagination">
              <button className="secondary-button" type="button" disabled={workspace.loadingMore} onClick={() => void workspace.loadMore()}>
                {t(workspace.loadingMore ? "learning.inbox.loadingMore" : "learning.inbox.loadMore")}
              </button>
            </footer>
          )}
        </div>
      </aside>
      <section
        className="learning-detail-pane"
        ref={detailPaneRef}
        tabIndex={-1}
        aria-label={t("learning.detail.views")}
      >
        {item ? (
          <LearningItemDetail
            key={item.id}
            item={item}
            workspace={workspace}
            ankiEnabled={ankiEnabled}
            ankiAvailable={ankiAvailable}
            onOpenAiSettings={onOpenAiSettings}
            view={view}
            onViewChange={setView}
            onBack={returnToList}
            scrollRef={detailScrollRef}
            onScroll={(event) => {
              if (event.currentTarget.offsetParent) scrollPositions.current[scrollKey] = event.currentTarget.scrollTop;
            }}
          />
        ) : (
          <div className="learning-empty"><BookOpenText size={22} /><p>{t("learning.inbox.selectHint")}</p></div>
        )}
      </section>
      {tooltip && createPortal(
        <div
          ref={tooltipRef}
          className={`settings-tab-tooltip${tooltip.keyboard ? " keyboard" : ""}`}
          role="tooltip"
          aria-hidden="true"
          style={{ top: tooltip.top, left: tooltip.left, maxWidth: Math.min(220, interfaceLayoutPixels(window.innerWidth) - 24) }}
        >
          {tooltip.label}
        </div>,
        document.body,
      )}
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
  const title = item.selected_text || item.working_text.split(/\r?\n/, 1)[0]?.trim();
  const context = item.selected_text ? item.working_text : item.working_text.slice(title?.length ?? 0).trim();
  const preview = item.source_translation || context;
  return (
    <button className={`learning-item-row ${active ? "active" : ""}`} type="button" aria-current={active ? "true" : undefined} onClick={onSelect}>
      <strong>{title}</strong>
      {preview && preview !== title && <p>{preview}</p>}
      <span className="learning-item-row-meta">
        <span>{t(`learning.kinds.${item.kind}`)}</span>
        <span className={`learning-status learning-status-${item.status}`}>{t(`learning.statuses.${item.status}`)}</span>
        <time>{timestamp(item.updated_at, locale)}</time>
      </span>
    </button>
  );
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
