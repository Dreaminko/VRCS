import { memo, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { BookOpenText, CalendarDays, Check, Languages, LoaderCircle, PlusCircle, Search } from "lucide-react";

import { conversationTime, timestamp } from "../../app/app-utils";
import type { LookupOrigin } from "../../app/app-types";
import type { ConversationSummary } from "../../conversations/conversations";
import type { LearningWorkspaceController } from "../hooks/useLearningWorkspace";
import { subtitleLearningKey, subtitleSelectionLearningKey } from "../../learning";
import { useTranslationPartial } from "../../realtime-state";
import type { Subtitle } from "../../types";
import { contentLanguageTag } from "../../app/ui-language";
import { DropdownField } from "../../shared/ui/DropdownField";

export function LearningSourceBrowser({
  conversation,
  subtitles,
  workspace,
  onSelect,
  onTranslate,
  translatingSubtitleIds = [],
  hasOlder,
  loading,
  loadingOlder,
  onLoadOlder,
  onCollected,
}: {
  conversation: ConversationSummary | undefined;
  subtitles: Subtitle[];
  workspace: LearningWorkspaceController;
  onSelect: (context: string, origin?: LookupOrigin) => Promise<void>;
  onTranslate?: (subtitleId: number) => void;
  translatingSubtitleIds?: number[];
  hasOlder: boolean;
  loading: boolean;
  loadingOlder: boolean;
  onLoadOlder: () => Promise<void>;
  onCollected: () => void;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const [language, setLanguage] = useState("all");
  const [range, setRange] = useState("all");
  const [source, setSource] = useState("all");
  const [query, setQuery] = useState("");
  const [selectedIds, setSelectedIds] = useState<Set<number>>(() => new Set());
  const lastSelectedIndexRef = useRef<number | null>(null);
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

  useEffect(() => {
    const visible = new Set(filtered.map((subtitle) => subtitle.id as number));
    setSelectedIds((current) => {
      const next = new Set([...current].filter((id) => visible.has(id)));
      const unchanged = next.size === current.size && [...next].every((id) => current.has(id));
      return unchanged ? current : next;
    });
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

  return (
    <section className="history-surface learning-material-surface">
      <div className="history-toolbar learning-material-toolbar">
        <div><h2>{t("learning.materials.title")}</h2></div>
        <div className="learning-material-search">
          <Search size={15} />
          <input value={query} type="search" placeholder={t("learning.materials.searchPlaceholder")} aria-label={t("learning.materials.search")} onChange={(event) => setQuery(event.target.value)} />
        </div>
        <div className="history-filters">
          <DropdownField
            compact
            icon={<Languages size={15} />}
            label={t("learning.language")}
            value={language}
            options={[
              { value: "all", label: t("languages.all") },
              { value: "ja", label: t("languages.japanese") },
              { value: "en", label: t("languages.english") },
              { value: "zh", label: t("languages.chinese") },
              { value: "ko", label: t("languages.korean") },
            ]}
            onChange={setLanguage}
          />
          <DropdownField
            compact
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
        <span>{selectedIds.size ? t("learning.materials.selected", { count: selectedIds.size }) : t("learning.materials.selectionHint")}</span>
        <div>
          <button type="button" disabled={!filtered.length} onClick={() => setSelectedIds(new Set(filtered.map((subtitle) => subtitle.id as number)))}>{t("learning.materials.selectAll")}</button>
          <button type="button" disabled={!selectedIds.size} onClick={() => setSelectedIds(new Set())}>{t("learning.materials.clearSelection")}</button>
          <button
            className="primary-button"
            type="button"
            disabled={!selectedIds.size || selectionBusy}
            onClick={() => void workspace.collectSubtitles(subtitles, selectedIds).then((item) => {
              if (!item) return;
              setSelectedIds(new Set());
              onCollected();
            })}
          ><PlusCircle size={15} />{t(selectionBusy ? "learning.actions.adding" : "learning.actions.addSelected")}</button>
        </div>
      </div>
      {conversation && filtered.length ? (
        <div className="history-list learning-material-list">
          <section className="learning-material-group">
            <header>
              <div>
                <strong>{conversation.title}</strong>
                <span>{t("learning.materials.conversationCount", { count: filtered.length })}</span>
              </div>
              <time>{conversationTime(
                conversation.startedAt,
                locale,
                t("date.today"),
                t("date.yesterday"),
              )}</time>
            </header>
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
  onCollected: () => void;
}) {
  const { t } = useTranslation();
  const translationPartial = useTranslationPartial(subtitle.id);
  const visibleTranslation = translationPartial ?? subtitle.translation_partial ?? subtitle.translations.at(-1);
  const key = subtitleLearningKey(subtitle);
  const collecting = workspace.isCollecting(key);
  const captured = workspace.isCaptured(key);
  const origin: LookupOrigin = {
    id: subtitle.id,
    language: subtitle.language,
    source: subtitle.source ?? null,
    createdAt: subtitle.created_at,
    translation: visibleTranslation?.text ?? null,
  };
  return (
    <article className={selected ? "selected" : ""}>
      <input
        className="learning-material-checkbox"
        type="checkbox"
        checked={selected}
        readOnly
        aria-label={t("learning.materials.selectSubtitle")}
        onClick={(event) => subtitle.id !== null && onToggle(subtitle.id, index, event.shiftKey)}
      />
      <time>{timestamp(subtitle.created_at, locale)}</time>
      <div className="learning-material-copy">
        <p lang={contentLanguageTag(subtitle.language)} onMouseUp={() => void onSelect(subtitle.text, origin)}>{subtitle.text}</p>
        {visibleTranslation && (
          <p className={translationPartial || subtitle.translation_partial ? "history-translation streaming-translation" : "history-translation"}>
            {visibleTranslation.text}
            {(translationPartial || subtitle.translation_partial) && <span className="streaming-ellipsis" aria-hidden="true">…</span>}
          </p>
        )}
      </div>
      <span>{subtitle.language?.toUpperCase() ?? "—"}</span>
      <div className="learning-material-actions">
        {onTranslate && subtitle.id !== null && (
          <button className="translation-action" type="button" disabled={translating} onClick={() => onTranslate(subtitle.id!)}>
            <Languages size={13} />{t(translating ? "translation.translating" : subtitle.translations.length ? "translation.retry" : "translation.action")}
          </button>
        )}
        <button
          className="learning-add-action"
          type="button"
          disabled={collecting || captured}
          onMouseUp={(event) => event.stopPropagation()}
          onClick={() => void workspace.collectSubtitle(subtitle).then((item) => item && onCollected())}
        >
          {captured ? <Check size={13} /> : <PlusCircle size={13} />}
          {t(captured ? "learning.actions.added" : collecting ? "learning.actions.adding" : "learning.actions.add")}
        </button>
      </div>
    </article>
  );
});
