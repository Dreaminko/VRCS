import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowLeft,
  AudioLines,
  BookOpenText,
  CalendarDays,
  Check,
  CheckCheck,
  Headphones,
  Languages,
  MessageSquare,
  Mic,
  PlusCircle,
  RotateCcw,
  Search,
  X,
} from "lucide-react";

import { conversationTime, timestamp } from "../../app/app-utils";
import type { LookupOrigin } from "../../app/app-types";
import { conversationViewKey } from "../../conversations/conversation-state";
import type { ConversationSummary } from "../../conversations/conversations";
import { useConversationTransition } from "../../conversations/useConversationTransition";
import type { LearningWorkspaceController } from "../hooks/useLearningWorkspace";
import type { LearningItem } from "../types";
import { subtitleLearningKey, subtitleSelectionLearningKey } from "../learning";
import { useTranslationPartials } from "../../core-client/realtime-state";
import type { Subtitle } from "../../subtitles/types";
import { contentLanguageTag } from "../../app/ui-language";
import { DropdownField } from "../../shared/ui/DropdownField";

const KNOWN_LANGUAGE_LABEL_KEYS: Record<string, string> = {
  ja: "languages.japanese",
  en: "languages.english",
  zh: "languages.chinese",
  ko: "languages.korean",
  fr: "languages.french",
  de: "languages.german",
  es: "languages.spanish",
};

export const LearningSourceBrowser = memo(function LearningSourceBrowser({
  conversation,
  conversations = [],
  activeConversationId,
  onSelectConversation,
  subtitles,
  workspace,
  onSelect,
  onTranslate,
  translatingSubtitleIds = [],
  hasOlder,
  loading,
  loadingOlder,
  onLoadOlder,
  targetViewKey,
  committedViewKey,
  onBackToInbox,
  collectedId = null,
  onOpenCollected,
  onDismissCollected,
  onOpenItem,
  onCollected,
}: {
  conversation: ConversationSummary | undefined;
  conversations?: ConversationSummary[];
  activeConversationId?: string;
  onSelectConversation?: (id: string) => void;
  subtitles: Subtitle[];
  workspace: LearningWorkspaceController;
  onSelect: (context: string, origin?: LookupOrigin) => Promise<void>;
  onTranslate?: (subtitleId: number) => void;
  translatingSubtitleIds?: number[];
  hasOlder: boolean;
  loading: boolean;
  loadingOlder: boolean;
  onLoadOlder: () => Promise<void>;
  targetViewKey?: string | null;
  committedViewKey?: string | null;
  onBackToInbox: () => void;
  collectedId?: number | null;
  onOpenCollected?: () => void;
  onDismissCollected?: () => void;
  onOpenItem?: (item: LearningItem) => void;
  onCollected: (item: LearningItem) => void;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const [language, setLanguage] = useState("all");
  const [range, setRange] = useState("all");
  const [source, setSource] = useState("all");
  const [query, setQuery] = useState("");
  const [selectedIds, setSelectedIds] = useState<Set<number>>(() => new Set());
  const lastSelectedIndexRef = useRef<number | null>(null);

  const conversationIds = useMemo(
    () => conversations.map((item) => item.id),
    [conversations],
  );
  const effectiveTargetKey = targetViewKey !== undefined
    ? targetViewKey
    : conversationViewKey(conversation?.id ?? null);
  const effectiveCommittedKey = committedViewKey !== undefined
    ? committedViewKey
    : conversationViewKey(loading ? null : (conversation?.id ?? null));
  const stage = useConversationTransition({
    targetKey: effectiveTargetKey,
    committedKey: effectiveCommittedKey,
    targetId: conversation?.id ?? null,
    committedId: effectiveCommittedKey === null || loading ? null : (conversation?.id ?? null),
    orderedIds: conversationIds,
    loading,
    data: { conversation, subtitles, hasOlder },
  });
  const displayedConversation = stage.displayedData.conversation;
  const displayedSubtitles = stage.displayedData.subtitles;
  const displayedHasOlder = stage.displayedData.hasOlder;

  const languageOptions = useMemo(() => {
    const codes = new Set<string>(["ja", "en", "zh", "ko"]);
    for (const subtitle of displayedSubtitles) {
      if (subtitle.language) codes.add(subtitle.language);
    }
    return [
      { value: "all", label: t("languages.all") },
      ...[...codes].map((code) => ({
        value: code,
        label: KNOWN_LANGUAGE_LABEL_KEYS[code] ? t(KNOWN_LANGUAGE_LABEL_KEYS[code]) : code.toUpperCase(),
      })),
    ];
  }, [displayedSubtitles, t]);

  const filtered = useMemo(() => {
    const now = Date.now();
    const normalizedQuery = query.trim().toLocaleLowerCase();
    return displayedSubtitles.filter((subtitle) => {
      if (subtitle.id === null) return false;
      if (language !== "all" && subtitle.language !== language) return false;
      if (source !== "all" && (subtitle.source ?? "speaker") !== source) return false;
      if (range === "today" && now - new Date(subtitle.created_at).getTime() > 86_400_000) return false;
      if (range === "week" && now - new Date(subtitle.created_at).getTime() > 604_800_000) return false;
      if (normalizedQuery) {
        const searchable = [subtitle.text, ...subtitle.translations.map((translation) => translation.text)]
          .join("\n")
          .toLocaleLowerCase();
        if (!searchable.includes(normalizedQuery)) return false;
      }
      return true;
    });
  }, [displayedSubtitles, language, query, range, source]);
  const indexById = useMemo(
    () => new Map(filtered.map((subtitle, index) => [subtitle.id as number, index])),
    [filtered],
  );

  // Retain selections across search/filter refinements within the same conversation,
  // and only prune IDs that no longer exist in the conversation's subtitle list.
  useEffect(() => {
    const available = new Set(
      displayedSubtitles.flatMap((subtitle) => (subtitle.id === null ? [] : [subtitle.id])),
    );
    setSelectedIds((current) => {
      const next = new Set([...current].filter((id) => available.has(id)));
      const unchanged = next.size === current.size && [...next].every((id) => current.has(id));
      return unchanged ? current : next;
    });
  }, [displayedSubtitles]);

  useEffect(() => {
    setSelectedIds((current) => (current.size === 0 ? current : new Set()));
  }, [conversation?.id]);

  useEffect(() => {
    lastSelectedIndexRef.current = null;
  }, [filtered]);

  const filteredRef = useRef(filtered);
  filteredRef.current = filtered;
  const toggleSelection = useCallback((id: number, index: number, rangeSelection: boolean) => {
    setSelectedIds((current) => {
      const next = new Set(current);
      if (rangeSelection && lastSelectedIndexRef.current !== null) {
        const start = Math.min(lastSelectedIndexRef.current, index);
        const end = Math.max(lastSelectedIndexRef.current, index);
        const shouldSelect = !current.has(id);
        for (const subtitle of filteredRef.current.slice(start, end + 1)) {
          if (subtitle.id === null) continue;
          if (shouldSelect) next.add(subtitle.id);
          else next.delete(subtitle.id);
        }
      } else if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
    lastSelectedIndexRef.current = index;
  }, []);
  const translatingIds = useMemo(() => new Set(translatingSubtitleIds), [translatingSubtitleIds]);

  const selectionKey = subtitleSelectionLearningKey(selectedIds);
  const selectionBusy = workspace.isCollecting(selectionKey);
  const allFilteredSelected = useMemo(() => {
    let selectableCount = 0;
    for (const subtitle of filtered) {
      if (subtitle.id === null) continue;
      selectableCount += 1;
      if (!selectedIds.has(subtitle.id)) return false;
    }
    return selectableCount > 0;
  }, [filtered, selectedIds]);
  const conversationOptions = useMemo(
    () => conversations.map((item) => ({
      value: item.id,
      label: item.title,
      description: [
        item.id === activeConversationId ? t("conversations.current") : null,
        conversationTime(item.startedAt, locale, t("date.today"), t("date.yesterday")),
        t("conversations.subtitleCount", { count: item.subtitleCount }),
      ].filter(Boolean).join(" · "),
    })),
    [activeConversationId, conversations, locale, t],
  );

  return (
    <section className="history-surface learning-material-surface">
      <div className="history-toolbar learning-material-toolbar">
        <button className="secondary-button learning-back-button" type="button" onClick={onBackToInbox}>
          <ArrowLeft size={15} />
          {t("learning.inbox.back")}
        </button>
        <div className="learning-material-search">
          <Search size={15} />
          <input
            value={query}
            type="search"
            placeholder={t("learning.materials.searchPlaceholder")}
            aria-label={t("learning.materials.search")}
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
        <div className="history-filters">
          <DropdownField
            compact
            floating
            icon={<Languages size={15} />}
            label={t("learning.language")}
            value={language}
            options={languageOptions}
            onChange={setLanguage}
          />
          <DropdownField
            compact
            floating
            icon={
              source === "microphone" ? (
                <Mic size={15} />
              ) : source === "chatbox" || source === "vrchat_chatbox" ? (
                <MessageSquare size={15} />
              ) : source === "speaker" ? (
                <Headphones size={15} />
              ) : (
                <AudioLines size={15} />
              )
            }
            label={t("learning.materials.source")}
            value={source}
            options={[
              { value: "all", label: t("learning.materials.sources.all") },
              { value: "speaker", label: t("learning.materials.sources.speaker") },
              { value: "microphone", label: t("learning.materials.sources.microphone") },
              { value: "chatbox", label: t("learning.materials.sources.chatbox") },
              { value: "vrchat_chatbox", label: `VRChat ${t("chatbox.title")}` },
            ]}
            onChange={setSource}
          />
          <DropdownField
            compact
            floating
            icon={<CalendarDays size={15} />}
            label={t("learning.dateRange")}
            value={range}
            options={[
              { value: "all", label: t("learning.allTime") },
              { value: "today", label: t("date.today") },
              { value: "week", label: t("learning.lastSevenDays") },
            ]}
            onChange={setRange}
          />
        </div>
      </div>
      <div className="learning-selection-bar">
        <div className="learning-selection-status">
          {conversation && (
            <>
              <div className="learning-material-session-picker">
                <DropdownField
                  compact
                  floating
                  floatingWidth={280}
                  icon={<MessageSquare size={15} />}
                  label={t("conversations.title")}
                  value={conversation.id}
                  options={conversationOptions.length ? conversationOptions : [{ value: conversation.id, label: conversation.title }]}
                  disabled={!onSelectConversation || conversationOptions.length <= 1}
                  onChange={(id) => onSelectConversation?.(id)}
                />
              </div>
              <span className="learning-material-count">
                {t("learning.materials.conversationCount", { count: filtered.length })}
              </span>
            </>
          )}
          {selectedIds.size > 0 && (
            <span>{t("learning.materials.selected", { count: selectedIds.size })}</span>
          )}
          {collectedId !== null && (
            <div className="learning-collection-feedback" role="status">
              <span><Check size={13} />{t("learning.actions.added")}</span>
              {onOpenCollected && (
                <button className="learning-collection-open" type="button" onClick={onOpenCollected}>
                  {t("learning.materials.openCollected")}
                </button>
              )}
              {onDismissCollected && (
                <button
                  className="learning-collection-dismiss"
                  type="button"
                  aria-label={t("common.close")}
                  onClick={onDismissCollected}
                >
                  <X size={12} />
                </button>
              )}
            </div>
          )}
        </div>
        <div className="learning-selection-actions">
          <button
            className="secondary-button"
            type="button"
            disabled={!filtered.length || allFilteredSelected}
            onClick={() => setSelectedIds((current) => {
              const next = new Set(current);
              for (const subtitle of filtered) {
                if (subtitle.id !== null) next.add(subtitle.id);
              }
              return next;
            })}
          >
            <CheckCheck size={14} />
            {t("learning.materials.selectAll")}
          </button>
          <button
            className="secondary-button"
            type="button"
            disabled={!selectedIds.size}
            onClick={() => setSelectedIds(new Set())}
          >
            <RotateCcw size={13} />
            {t("learning.materials.clearSelection")}
          </button>
          <button
            className="primary-button"
            type="button"
            disabled={!selectedIds.size || selectionBusy}
            onClick={() => void workspace.collectSubtitles(displayedSubtitles, selectedIds).then((item) => {
              if (!item) return;
              setSelectedIds(new Set());
              onCollected(item);
            })}
          ><PlusCircle size={15} />{t(selectionBusy ? "learning.actions.adding" : "learning.actions.addSelected")}</button>
        </div>
      </div>
      <div className="learning-material-scroll" data-floating-boundary>
        <div
          className="learning-material-stage"
          data-transition-phase={stage.phase}
          data-transition-direction={stage.direction}
        >
          {stage.phase === "waiting" ? null : stage.phase === "skeleton" ? (
            <div className="learning-material-skeleton" role="status" aria-label={t("common.loading")}>
              {["64%", "48%", "70%", "54%"].map((width, index) => (
                <div key={index} className="learning-material-skeleton-row" aria-hidden="true">
                  <span className="learning-material-skeleton-bar" style={{ width: "58px" }} />
                  <div className="learning-material-skeleton-copy">
                    <span className="learning-material-skeleton-bar" style={{ width }} />
                    <span className="learning-material-skeleton-bar" style={{ width: "38%" }} />
                  </div>
                  <span className="learning-material-skeleton-bar" style={{ width: "68px" }} />
                </div>
              ))}
            </div>
          ) : displayedConversation && filtered.length ? (
            <div className="history-list learning-material-list">
              <section className="learning-material-group">
                <div>
                  {filtered.map((subtitle) => (
                    <LearningMaterialRow
                      key={subtitle.id ?? subtitle.created_at}
                      subtitle={subtitle}
                      index={subtitle.id === null ? 0 : indexById.get(subtitle.id) ?? 0}
                      locale={locale}
                      selected={subtitle.id !== null && selectedIds.has(subtitle.id)}
                      onToggle={toggleSelection}
                      onSelect={onSelect}
                      onTranslate={onTranslate}
                      translating={subtitle.id !== null && translatingIds.has(subtitle.id)}
                      collectSubtitle={workspace.collectSubtitle}
                      isCollecting={workspace.isCollecting}
                      isCaptured={workspace.isCaptured}
                      onOpenItem={onOpenItem}
                      onCollected={onCollected}
                    />
                  ))}
                </div>
              </section>
            </div>
          ) : <div className="empty-state"><BookOpenText size={22} /><p>{t("learning.empty")}</p></div>}
          {displayedHasOlder && stage.phase !== "waiting" && stage.phase !== "skeleton" && (
            <div className="history-load-older">
              <button className="secondary-button" type="button" disabled={loadingOlder} onClick={() => void onLoadOlder()}>
                {t(loadingOlder ? "learning.loadingEarlier" : "learning.loadEarlier")}
              </button>
            </div>
          )}
        </div>
      </div>
    </section>
  );
});

const LearningMaterialRow = memo(function LearningMaterialRow({
  subtitle,
  index,
  locale,
  selected,
  onToggle,
  onSelect,
  onTranslate,
  translating,
  collectSubtitle,
  isCollecting,
  isCaptured,
  onOpenItem,
  onCollected,
}: {
  subtitle: Subtitle;
  index: number;
  locale: string;
  selected: boolean;
  onToggle: (id: number, index: number, rangeSelection: boolean) => void;
  onSelect: (context: string, origin?: LookupOrigin) => Promise<void>;
  onTranslate?: (subtitleId: number) => void;
  translating: boolean;
  collectSubtitle: LearningWorkspaceController["collectSubtitle"];
  isCollecting: LearningWorkspaceController["isCollecting"];
  isCaptured: LearningWorkspaceController["isCaptured"];
  onOpenItem?: (item: LearningItem) => void;
  onCollected: (item: LearningItem) => void;
}) {
  const { t } = useTranslation();
  const translationPartial = useTranslationPartials(subtitle.id)[0];
  const visibleTranslation = translationPartial ?? subtitle.translation_partial ?? subtitle.translations[0];
  const key = subtitleLearningKey(subtitle);
  const collecting = isCollecting(key);
  const captured = isCaptured(key);
  const sourceKind = subtitle.source ?? "speaker";
  const sourceLabel = sourceKind === "vrchat_chatbox"
    ? `VRChat ${t("chatbox.title")}`
    : t(`learning.materials.sources.${sourceKind}`);
  const origin: LookupOrigin = {
    id: subtitle.id,
    language: subtitle.language,
    source: subtitle.source ?? null,
    createdAt: subtitle.created_at,
    translation: visibleTranslation?.text ?? null,
  };
  const handleToggle = (shiftKey: boolean) => {
    if (subtitle.id !== null) onToggle(subtitle.id, index, shiftKey);
  };
  return (
    <article className={selected ? "selected" : ""}>
      <input
        className="learning-material-checkbox"
        type="checkbox"
        checked={selected}
        readOnly
        aria-label={t("learning.materials.selectSubtitle")}
        onClick={(event) => handleToggle(event.shiftKey)}
      />
      <div
        className="learning-material-meta"
        data-tooltip={sourceLabel}
        onClick={(event) => handleToggle(event.shiftKey)}
      >
        <span className={`learning-material-source learning-material-source-${sourceKind}`} aria-label={sourceLabel}>
          {sourceKind === "microphone" ? (
            <Mic size={11} />
          ) : sourceKind === "chatbox" || sourceKind === "vrchat_chatbox" ? (
            <MessageSquare size={11} />
          ) : (
            <Headphones size={11} />
          )}
        </span>
        <time>{timestamp(subtitle.created_at, locale)}</time>
      </div>
      <div className="learning-material-copy">
        <p lang={contentLanguageTag(subtitle.language)} onMouseUp={() => void onSelect(subtitle.text, origin)}>{subtitle.text}</p>
        {visibleTranslation && (
          <p className={translationPartial || subtitle.translation_partial ? "history-translation streaming-translation" : "history-translation"}>
            {visibleTranslation.text}
            {(translationPartial || subtitle.translation_partial) && <span className="streaming-ellipsis" aria-hidden="true">…</span>}
          </p>
        )}
      </div>
      <div className="learning-material-actions">
        {onTranslate && subtitle.id !== null && (
          <button className="translation-action" type="button" disabled={translating} onClick={() => onTranslate(subtitle.id!)}>
            <Languages size={13} />{t(translating ? "translation.translating" : subtitle.translations.length ? "translation.retry" : "translation.action")}
          </button>
        )}
        <button
          className={`learning-add-action${captured ? " captured" : ""}`}
          type="button"
          disabled={collecting}
          data-tooltip={captured ? t("learning.materials.openCollected") : undefined}
          onMouseUp={(event) => event.stopPropagation()}
          onClick={() => void collectSubtitle(subtitle).then((item) => {
            if (!item) return;
            if (captured && onOpenItem) onOpenItem(item);
            else onCollected(item);
          })}
        >
          {captured ? <Check size={13} /> : <PlusCircle size={13} />}
          {t(captured ? "learning.actions.added" : collecting ? "learning.actions.adding" : "learning.actions.add")}
        </button>
      </div>
    </article>
  );
});
