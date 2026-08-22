import { useMemo } from "react";

import { useConversationHistory } from "../../conversations/useConversationHistory";
import { useCoreEventStream } from "../../core-client/useCoreEventStream";

export function useSubtitleStream({
  coreConfigured,
  reportError,
  clearErrorFrom,
}: {
  coreConfigured: boolean;
  reportError: (
    reason: unknown,
    fallbackKey: string,
    source?: string,
  ) => void;
  clearErrorFrom: (source: string) => void;
}) {
  const history = useConversationHistory({
    coreConfigured,
    reportError,
    clearErrorFrom,
  });
  const handlers = useMemo(() => ({
    onConnected: () => {
      void history.refreshOpenConversation();
    },
    onSubtitle: history.receiveSubtitle,
    onTranslationStarted: history.translationStarted,
    onTranslationCompleted: history.translationCompleted,
    onTranslationFailed: history.translationFailed,
  }), [
    history.receiveSubtitle,
    history.refreshOpenConversation,
    history.translationCompleted,
    history.translationFailed,
    history.translationStarted,
  ]);
  const stream = useCoreEventStream({
    coreConfigured,
    handlers,
    reportError,
    clearErrorFrom,
  });

  return {
    connection: stream.connection,
    openedConversationId: history.openedConversationId,
    subtitles: history.subtitles,
    hasOlderSubtitles: history.hasOlderSubtitles,
    loadingConversationSubtitles: history.loadingConversationSubtitles,
    loadingOlderSubtitles: history.loadingOlderSubtitles,
    focusedSubtitleId: history.focusedSubtitleId,
    conversationCatalogEvent: stream.conversationCatalogEvent,
    openConversation: history.openConversation,
    openConversationAt: history.openConversationAt,
    loadOlderSubtitles: history.loadOlderSubtitles,
    vrchatMuteStatus: stream.vrchatMuteStatus,
    translatingSubtitleIds: history.translatingSubtitleIds,
    clearPartials: stream.clearPartials,
    translateSubtitle: history.translateSubtitle,
  };
}
