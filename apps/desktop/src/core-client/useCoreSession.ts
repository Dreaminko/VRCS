import { useEffect, useRef } from "react";
import { useSubtitleStream } from "../subtitles/hooks/useSubtitleStream";
import { useCaptureRuntime } from "../capture/useCaptureRuntime";
import { useDictionaryRuntime } from "../dictionary/useDictionaryRuntime";
import { useSettingsRuntime } from "../settings/hooks/useSettingsRuntime";
import { subscribeCoreStreamResync } from "./useCoreEventStream";
import { useCoreRuntime } from "./useCoreRuntime";
import { useRuntimeErrors } from "./useRuntimeErrors";
import { usePlayerAudioStart } from "../capture/usePlayerAudioStart";

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
  const playerAudioStart = usePlayerAudioStart({
    os: runtime.capabilities.desktop?.os ?? runtime.capabilities.core?.os,
    beforeMemoryChange: async () => {
      const current = runtime.health.getCurrent();
      if (current?.capture_requested && !await capture.setRunning(false)) return false;
      if (current?.network_capture_requested && !await capture.setNetworkRunning(false)) return false;
      return true;
    },
  });
  const settings = useSettingsRuntime({
    active: resourcesActive,
    coreConfigured: runtime.coreConfigured,
    audioSupported: runtime.capabilities.core
      ? runtime.capabilities.core.audio.microphone.supported || runtime.capabilities.core.audio.system_output.supported
      : undefined,
    refreshCapabilities: runtime.refreshCapabilities,
    health: runtime.health,
    stopMicrophoneTest: capture.stopMicrophoneTest,
    setCaptureRunning: capture.setRunning,
    playerAudioStart,
    clearErrorFrom: errors.clearErrorFrom,
    reportError: errors.reportError,
  });
  const dictionary = useDictionaryRuntime({
    active: resourcesActive && Boolean(settings.value?.features.learning),
    coreConfigured: runtime.coreConfigured && Boolean(settings.value?.features.learning),
    clearErrorFrom: errors.clearErrorFrom,
    reportError: errors.reportError,
  });

  const resyncRef = useRef({
    refreshHealth: runtime.health.refreshQuietly,
    refreshCapabilities: runtime.refreshCapabilities,
    refreshSettings: settings.refresh,
  });
  resyncRef.current = {
    refreshHealth: runtime.health.refreshQuietly,
    refreshCapabilities: runtime.refreshCapabilities,
    refreshSettings: settings.refresh,
  };
  useEffect(() => subscribeCoreStreamResync(() => {
    void resyncRef.current.refreshHealth();
    void resyncRef.current.refreshCapabilities();
    void resyncRef.current.refreshSettings().catch(() => undefined);
  }), []);

  return {
    runtime: {
      connection: runtime.startupFailed ? "disconnected" as const : subtitles.connection,
      ready: runtime.ready,
      startupFailed: runtime.startupFailed,
      health: runtime.health.value,
      capabilities: runtime.capabilities,
      vrchatMuteStatus: subtitles.vrchatMuteStatus
        ?? runtime.health.value?.vrchat_mute_sync
        ?? null,
      error: errors.error,
      errorCode: errors.errorCode,
      retry: runtime.retry,
      reportError: errors.reportError,
      clearError: errors.clearError,
      clearErrorFrom: errors.clearErrorFrom,
    },
    capture: { ...capture, playerAudioStart },
    settings,
    dictionary,
    subtitles: {
      openedConversationId: subtitles.openedConversationId,
      targetViewKey: subtitles.targetViewKey,
      committedViewKey: subtitles.committedViewKey,
      items: subtitles.subtitles,
      hasOlder: subtitles.hasOlderSubtitles,
      loadingConversation: subtitles.loadingConversationSubtitles,
      loadingOlder: subtitles.loadingOlderSubtitles,
      focusedSubtitleId: subtitles.focusedSubtitleId,
      conversationCatalogEvent: subtitles.conversationCatalogEvent,
      openConversation: subtitles.openConversation,
      openConversationAt: subtitles.openConversationAt,
      loadOlder: subtitles.loadOlderSubtitles,
      translate: subtitles.translateSubtitle,
      translatingIds: subtitles.translatingSubtitleIds,
    },
  };
}
