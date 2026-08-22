import { LoaderCircle, SearchX } from "lucide-react";
import { useTranslation } from "react-i18next";

import { timestamp } from "../app/app-utils";
import { contentLanguageTag } from "../app/ui-language";
import type { SubtitleSearchHit } from "../subtitles/types";
import type { ConversationSummary } from "./conversations";
import { subtitleSearchMatchRange } from "../subtitle-search";

export function ConversationSearchResults({
  query,
  items,
  conversations,
  loading,
  loadingMore,
  hasMore,
  failed,
  searchable,
  onLoadMore,
  onSelect,
}: {
  query: string;
  items: SubtitleSearchHit[];
  conversations: ConversationSummary[];
  loading: boolean;
  loadingMore: boolean;
  hasMore: boolean;
  failed: boolean;
  searchable: boolean;
  onLoadMore: () => Promise<void>;
  onSelect: (conversationId: string, subtitleId: number) => void;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";

  if (!searchable) {
    return <p className="conversation-search-status">{t("conversations.searchHint")}</p>;
  }
  if (loading) {
    return (
      <p className="conversation-search-status" role="status">
        <LoaderCircle className="spinning" size={15} />
        {t("conversations.searching")}
      </p>
    );
  }
  if (failed && !items.length) {
    return <p className="conversation-search-status error" role="alert">{t("conversations.searchFailed")}</p>;
  }
  if (!items.length) {
    return (
      <p className="conversation-search-status">
        <SearchX size={16} />
        {t("conversations.searchEmpty")}
      </p>
    );
  }

  return (
    <div className="conversation-search-results" aria-label={t("conversations.searchResults")}>
      <p className="conversation-search-summary">{t("conversations.searchResultCount", { count: items.length })}</p>
      {items.map((hit) => {
        const subtitleId = hit.subtitle.id;
        const conversationId = hit.subtitle.conversation_id;
        const conversation = conversations.find((item) => item.id === conversationId);
        const matchedTranslation = hit.subtitle.translations.find(
          (translation) => translation.text === hit.matched_text,
        );
        return (
          <button
            className="conversation-search-result"
            type="button"
            key={`${conversationId}:${subtitleId}`}
            disabled={subtitleId === null || !conversationId}
            onClick={() => {
              if (subtitleId !== null && conversationId) onSelect(conversationId, subtitleId);
            }}
          >
            <span className="conversation-search-result-meta">
              <strong>{conversation?.title ?? t("conversations.untitled")}</strong>
              <time>{timestamp(hit.subtitle.created_at, locale)}</time>
            </span>
            <span
              className="conversation-search-result-match"
              lang={contentLanguageTag(
                hit.matched_field === "original"
                  ? hit.subtitle.language
                  : matchedTranslation?.target_language,
              )}
            >
              <HighlightedText text={hit.matched_text} query={query.trim()} />
            </span>
            {hit.matched_field === "translation" && (
              <span className="conversation-search-result-original" lang={contentLanguageTag(hit.subtitle.language)}>
                {hit.subtitle.text}
              </span>
            )}
          </button>
        );
      })}
      {failed && <p className="conversation-search-inline-error" role="alert">{t("conversations.searchFailed")}</p>}
      {hasMore && (
        <button
          className="conversation-search-load-more"
          type="button"
          disabled={loadingMore}
          onClick={() => void onLoadMore()}
        >
          {loadingMore && <LoaderCircle className="spinning" size={14} />}
          {t(loadingMore ? "conversations.searching" : "conversations.searchMore")}
        </button>
      )}
    </div>
  );
}

function HighlightedText({ text, query }: { text: string; query: string }) {
  const range = subtitleSearchMatchRange(text, query);
  if (!range) return text;
  return (
    <>
      {text.slice(0, range.start)}
      <mark>{text.slice(range.start, range.end)}</mark>
      {text.slice(range.end)}
    </>
  );
}
