import { memo, useEffect, useMemo, useRef, useState } from "react";
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
  LoaderCircle,
  MessageSquare,
  Mic,
  PlusCircle,
  RotateCcw,
  Search,
  X,
} from "lucide-react";

import { conversationTime, timestamp } from "../../app/app-utils";
import type { LookupOrigin } from "../../app/app-types";
import type { ConversationSummary } from "../../conversations/conversations";
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

export function LearningSourceBrowser({
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

  const languageOptions = useMemo(() => {
    const codes = new Set<string>(["ja", "en", "zh", "ko"]);
    for (const subtitle of subtitles) {
      if (subtitle.language) codes.add(subtitle.language);
    }
    return [
      { value: "all", label: t("languages.all") },
      ...[...codes].map((code) => ({
        value: code,
        label: KNOWN_LANGUAGE_LABEL_KEYS[code] ? t(KNOWN_LANGUAGE_LABEL_KEYS[code]) : code.toUpperCase(),
      })),
    ];
  }, [subtitles, t]);

  const filtered = useMemo(() => {
    const now = Date.now();
    const normalizedQuery = query.trim().toLocaleLowerCase();
    return subtitles.filter((subtitle) => {
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
  }, [language, query, range, source, subtitles]);
  const indexById = useMemo(
    () => new Map(filtered.map((subtitle, index) => [subtitle.id as number, index])),
    [filtered],
  );

  // Retain selections across search/filter refinements within the same conversation,
  // and only prune IDs that no longer exist in the conversation's subtitle list.
  useEffect(() => {
    const available = new Set(
      subtitles.flatMap((subtitle) => (subtitle.id === null ? [] : [subtitle.id])),
    );
    setSelectedIds((current) => {
      const next = new Set([...current].filter((id) => available.has(id)));
      const unchanged = next.size === current.size && [...next].every((id) => current.has(id));
      return unchanged ? current : next;
    });
  }, [subtitles]);

  useEffect(() => {
    lastSelectedIndexRef.current = null;
  }, [filtered]);

  const toggleSelection = (id: number, index: number, rangeSelection: boolean) => {
    setSelectedIds((current) => {
      const next = new Set(current);
      if (rangeSelection && lastSelectedIndexRef.current !== null) {
        const start = Math.min(lastSelectedIndexRef.current, index);
        const end = Math.max(lastSelectedIndexRef.current, index);
        const shouldSelect = !current.has(id);
        for (const subtitle of filtered.slice(start, end + 1)) {
          if (subtitle.id === null) continue;
          if (shouldSelect) next.add(subtitle.id);
          else next.delete(subtitle.id);
        }
      } else if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
    lastSelectedIndexRef.current = index;
  };

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
              ) : source === "chatbox" ? (
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
            onClick={() => void workspace.collectSubtitles(subtitles, selectedIds).then((item) => {
              if (!item) return;
              setSelectedIds(new Set());
              onCollected(item);
            })}
          ><PlusCircle size={15} />{t(selectionBusy ? "learning.actions.adding" : "learning.actions.addSelected")}</button>
        </div>
      </div>
      <div className="learning-material-scroll" data-floating-boundary>
        {conversation && filtered.length ? (
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
                    translating={subtitle.id !== null && translatingSubtitleIds.includes(subtitle.id)}
                    workspace={workspace}
                    onOpenItem={onOpenItem}
                    onCollected={onCollected}
                  />
                ))}
              </div>
            </section>
          </div>
        ) : loading ? (
          <div className="empty-state" role="status"><LoaderCircle className="spinning" size={22} /><p>{t("common.loading")}</p></div>
        ) : <div className="empty-state"><BookOpenText size={22} /><p>{t("learning.empty")}</p></div>}
        {hasOlder && (
          <div className="history-load-older">
            <button className="secondary-button" type="button" disabled={loadingOlder} onClick={() => void onLoadOlder()}>
              {t(loadingOlder ? "learning.loadingEarlier" : "learning.loadEarlier")}
            </button>
          </div>
        )}
      </div>
    </section>
  );
}

const LearningMaterialRow = memo(function LearningMaterialRow({
  subtitle,
  index,
  locale,
  selected,
  onToggle,
  onSelect,
  onTranslate,
  translating,
  workspace,
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
  workspace: LearningWorkspaceController;
  onOpenItem?: (item: LearningItem) => void;
  onCollected: (item: LearningItem) => void;
}) {
  const { t } = useTranslation();
  const translationPartial = useTranslationPartials(subtitle.id)[0];
  const visibleTranslation = translationPartial ?? subtitle.translation_partial ?? subtitle.translations[0];
  const key = subtitleLearningKey(subtitle);
  const collecting = workspace.isCollecting(key);
  const captured = workspace.isCaptured(key);
  const sourceKind = subtitle.source ?? "speaker";
  const sourceLabel = t(`learning.materials.sources.${sourceKind}`);
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
          ) : sourceKind === "chatbox" ? (
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
          onClick={() => void workspace.collectSubtitle(subtitle).then((item) => {
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
