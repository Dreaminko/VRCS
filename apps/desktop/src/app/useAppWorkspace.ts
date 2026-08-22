import { useCallback } from "react";
import { useTranslation } from "react-i18next";

import type { Page } from "./app-types";
import { subtitleForCompactView } from "../compact-mode";
import { useChatboxWorkspace } from "../chatbox/useChatboxWorkspace";
import { useConversationWorkspace } from "../conversations/useConversationWorkspace";
import { useCaptureControl } from "../core-client/useCaptureControl";
import { useCoreSession } from "../core-client/useCoreSession";
import { useCudaRuntimeWarning } from "../core-client/useCudaRuntimeWarning";
import { useLearningWorkspace } from "../learning/hooks/useLearningWorkspace";
import { useSubtitleLearningActions } from "../learning/hooks/useSubtitleLearningActions";
import { useOnboardingFlow } from "../onboarding/useOnboardingFlow";
import { useSelectionTools } from "../selection/useSelectionTools";
import { useApiProfileViews } from "../settings/hooks/useApiProfileViews";
import type { SettingsCategory } from "../settings/settings-types";
import { useCompactWindow } from "../shell/useCompactWindow";
import {
  supportsCustomTranslationLanguage,
  translationLanguageCodesForProfile,
} from "../translation-languages";
import { useAppUpdater } from "../updates/useAppUpdater";
import { useInterfaceScale } from "./useInterfaceScale";
import { useVrchatMuteToast } from "./useVrchatMuteToast";

export function useAppWorkspace({
  page,
  setPage,
  setSettingsInitialCategory,
}: {
  page: Page;
  setPage: (page: Page) => void;
  setSettingsInitialCategory: (category: SettingsCategory) => void;
}) {
  const { t, i18n } = useTranslation();
  const onboardingFlow = useOnboardingFlow();
  const updater = useAppUpdater(onboardingFlow.status === "complete");
  const core = useCoreSession(page === "settings" || onboardingFlow.status !== "complete");
  const { runtime, capture: captureRuntime, settings, dictionary, subtitles } = core;
  const compactWindow = useCompactWindow({
    clearErrorFrom: runtime.clearErrorFrom,
    reportError: runtime.reportError,
  });
  const selection = useSelectionTools({
    compact: compactWindow.compact,
    dictionaryLookupEnabled: settings.value?.dictionary.selection_lookup_enabled ?? true,
    resizeCompactWindow: compactWindow.resizeCompactWindow,
    reportError: runtime.reportError,
  });
  const learningWorkspace = useLearningWorkspace(page === "learning" && runtime.ready, runtime.ready);
  const openLearningPage = useCallback(() => setPage("learning"), [setPage]);
  const learningActions = useSubtitleLearningActions({
    workspace: learningWorkspace,
    clearLookup: selection.clear,
    openLearningPage,
  });
  const chatbox = useChatboxWorkspace(settings.value);
  const conversation = useConversationWorkspace({
    coreReady: runtime.ready,
    openedConversationId: subtitles.openedConversationId,
    subtitles: subtitles.items,
    conversationCatalogEvent: subtitles.conversationCatalogEvent,
    openConversation: subtitles.openConversation,
    page,
    running: runtime.health?.capture_requested ?? false,
    hasOlderSubtitles: subtitles.hasOlder,
    loadingConversationSubtitles: subtitles.loadingConversation,
    reportError: runtime.reportError,
    clearErrorFrom: runtime.clearErrorFrom,
  });
  const captureControl = useCaptureControl({
    coreReady: runtime.ready,
    running: runtime.health?.capture_requested ?? false,
    outputMode: settings.value?.audio.output.mode,
    compact: compactWindow.compact,
    createConversation: conversation.createConversation,
    clearLookup: selection.clear,
    toggleCoreCapture: captureRuntime.toggle,
    clearErrorFrom: runtime.clearErrorFrom,
    resizeCompactWindow: compactWindow.resizeCompactWindow,
    collapseCompactOverlay: compactWindow.collapseCompactOverlay,
    reportError: runtime.reportError,
  });
  const cudaRuntimeWarning = useCudaRuntimeWarning(
    settings.asr.capabilities,
    onboardingFlow.status === "complete",
  );
  const vrchatMuteToast = useVrchatMuteToast({
    settingsReady: settings.value !== null,
    enabled: settings.value?.osc.mute_status_toast_enabled ?? false,
    status: runtime.vrchatMuteStatus,
  });
  const { interfaceScale, setInterfaceScale } = useInterfaceScale(runtime.reportError);
  const chatboxRoute = settings.value?.translation.microphone_targets[0];
  const providerCatalog = useApiProfileViews(
    `${settings.value?.asr.active_profile_id ?? "local"}:${settings.value?.asr.backend ?? ""}:${chatboxRoute?.profile_id ?? ""}`,
  );
  const translationProfile = providerCatalog.profiles.find(
    (profile) => profile.id === chatboxRoute?.profile_id,
  );
  const translateVisibleSubtitle = useCallback((id: number) => {
    void subtitles.translate(id);
  }, [subtitles.translate]);
  const translationHandler = settings.value?.translation.mode === "disabled"
    ? undefined
    : translateVisibleSubtitle;
  const selectedConversationLoading = Boolean(
    subtitles.loadingConversation
    && conversation.selectedConversation?.id === subtitles.openedConversationId,
  );
  const compactSubtitle = subtitleForCompactView(
    subtitles.items,
    selection.target?.context,
  );

  const openSelectionAiSettings = useCallback(async () => {
    if (compactWindow.compact && !(await compactWindow.exitCompact())) return;
    selection.clear();
    setSettingsInitialCategory("learning");
    setPage("settings");
  }, [compactWindow, selection, setPage, setSettingsInitialCategory]);

  const finishOnboarding = useCallback(async (startCapture: boolean) => {
    await onboardingFlow.finish({
      startCapture,
      startCaptureAction: captureControl.toggleCapture,
      startCaptureError: t("onboarding.errors.startCapture"),
    });
    setPage("live");
  }, [captureControl.toggleCapture, onboardingFlow.finish, setPage, t]);

  return {
    locale: i18n.resolvedLanguage ?? "en-US",
    onboarding: {
      ...onboardingFlow,
      finish: finishOnboarding,
    },
    runtime: {
      ...runtime,
      cudaWarning: cudaRuntimeWarning,
      vrchatMuteToast,
    },
    capture: {
      ...captureRuntime,
      ...captureControl,
    },
    compact: {
      ...compactWindow,
      subtitle: compactSubtitle,
    },
    conversations: {
      ...conversation,
      selectedLoading: selectedConversationLoading,
    },
    subtitles: {
      ...subtitles,
      translationHandler,
    },
    learning: {
      workspace: learningWorkspace,
      actions: learningActions,
    },
    selection: {
      ...selection,
      openAiSettings: openSelectionAiSettings,
    },
    settings: {
      ...settings,
      interfaceScale,
      setInterfaceScale,
    },
    providers: {
      catalog: providerCatalog,
      translationProfile,
      translationLanguageCodes: translationLanguageCodesForProfile(translationProfile),
      allowCustomTranslationLanguage: supportsCustomTranslationLanguage(translationProfile),
    },
    chatbox,
    integrations: { updater },
    dictionary,
  };
}

export type AppWorkspace = ReturnType<typeof useAppWorkspace>;
