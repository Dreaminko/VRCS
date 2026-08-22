import { useSubtitleStream } from "../subtitles/hooks/useSubtitleStream";
import { useCaptureRuntime } from "../capture/useCaptureRuntime";
import { useDictionaryRuntime } from "../dictionary/useDictionaryRuntime";
import { useSettingsRuntime } from "../settings/hooks/useSettingsRuntime";
import { useCoreRuntime } from "./useCoreRuntime";
import { useRuntimeErrors } from "./useRuntimeErrors";

export function useCoreSession(resourcesActive: boolean) {
  const errors = useRuntimeErrors();
  const runtime = useCoreRuntime(errors);
  const subtitles = useSubtitleStream({
    coreConfigured: runtime.coreConfigured,
    reportError: errors.reportError,
    clearErrorFrom: errors.clearErrorFrom,
  });
  const capture = useCaptureRuntime({
    health: runtime.health,
    clearPartials: subtitles.clearPartials,
    clearErrorFrom: errors.clearErrorFrom,
    reportError: errors.reportError,
  });
  const settings = useSettingsRuntime({
    active: resourcesActive,
    coreConfigured: runtime.coreConfigured,
    health: runtime.health,
    stopMicrophoneTest: capture.stopMicrophoneTest,
    clearErrorFrom: errors.clearErrorFrom,
    reportError: errors.reportError,
  });
  const dictionary = useDictionaryRuntime({
    active: resourcesActive,
    coreConfigured: runtime.coreConfigured,
    clearErrorFrom: errors.clearErrorFrom,
    reportError: errors.reportError,
  });

  return {
    runtime: {
      connection: runtime.startupFailed ? "disconnected" as const : subtitles.connection,
      ready: runtime.ready,
      startupFailed: runtime.startupFailed,
      health: runtime.health.value,
      vrchatMuteStatus: subtitles.vrchatMuteStatus
        ?? runtime.health.value?.vrchat_mute_sync
        ?? null,
      error: errors.error,
      retry: runtime.retry,
      reportError: errors.reportError,
      clearError: errors.clearError,
      clearErrorFrom: errors.clearErrorFrom,
    },
    capture,
    settings,
    dictionary,
    subtitles: {
      openedConversationId: subtitles.openedConversationId,
      items: subtitles.subtitles,
      hasOlder: subtitles.hasOlderSubtitles,
      loadingConversation: subtitles.loadingConversationSubtitles,
      loadingOlder: subtitles.loadingOlderSubtitles,
      conversationCatalogEvent: subtitles.conversationCatalogEvent,
      openConversation: subtitles.openConversation,
      loadOlder: subtitles.loadOlderSubtitles,
      translate: subtitles.translateSubtitle,
      translatingIds: subtitles.translatingSubtitleIds,
    },
  };
}
