import { useCallback, useEffect, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";

import type { Page } from "./app-types";
import { subtitlesForCompactView } from "../shell/compact-mode";
import { useChatboxWorkspace } from "../chatbox/useChatboxWorkspace";
import { useConversationWorkspace } from "../conversations/useConversationWorkspace";
import { useCaptureControl } from "../core-client/useCaptureControl";
import { useCoreSession } from "../core-client/useCoreSession";
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
} from "../translation/translation-languages";
import { useAppUpdater } from "../updates/useAppUpdater";
import { useInterfaceScale } from "./useInterfaceScale";
import { usePlayerHistoryPreferences } from "../subtitles/usePlayerHistoryPreferences";
import { networkCaptureFeatureEnabled, usesVrchatNetwork } from "../capture/vrchat-network";
import { captureUnavailableReason, networkCaptureUnavailableReason } from "../settings/feature-availability";
import type { Subtitle } from "../subtitles/types";

const EMPTY_COMPACT_SUBTITLES: Subtitle[] = [];

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
  const previousOnboardingStatus = useRef(onboardingFlow.status);
  useEffect(() => {
    const previous = previousOnboardingStatus.current;
    previousOnboardingStatus.current = onboardingFlow.status;
    if (previous !== "complete" && onboardingFlow.status === "complete") {
      runtime.clearError();
    }
  }, [onboardingFlow.status, runtime.clearError]);
  const playerDisplay = usePlayerHistoryPreferences(settings.value?.translation, runtime.health?.language_session);
  const learningAvailable = settings.value?.features.learning ?? false;
  useEffect(() => {
    if (settings.value && !learningAvailable && page === "learning") setPage("live");
  }, [learningAvailable, page, setPage, settings.value]);
  const compactWindow = useCompactWindow({
    alwaysOnTopAvailable: runtime.capabilities.desktop?.always_on_top.available,
    clearErrorFrom: runtime.clearErrorFrom,
    reportError: runtime.reportError,
  });
  const selection = useSelectionTools({
    enabled: learningAvailable,
    dictionaryLookupEnabled: settings.value?.dictionary.selection_lookup_enabled ?? true,
    reportError: runtime.reportError,
  });
  const learningWorkspace = useLearningWorkspace(page === "learning" && runtime.ready && learningAvailable, runtime.ready && learningAvailable, settings.value);
  const openLearningPage = useCallback(() => setPage("learning"), [setPage]);
  const learningActions = useSubtitleLearningActions({
    workspace: learningWorkspace,
    clearLookup: selection.clear,
    openLearningPage,
  });
  const chatbox = useChatboxWorkspace(
    settings.value,
    runtime.health?.room_context?.scope_id ?? null,
    runtime.health?.language_session,
  );
  const conversation = useConversationWorkspace({
    coreReady: runtime.ready,
    openedConversationId: subtitles.openedConversationId,
    subtitles: subtitles.items,
    conversationCatalogEvent: subtitles.conversationCatalogEvent,
    openConversation: subtitles.openConversation,
    openConversationAt: subtitles.openConversationAt,
    page,
    hasOlderSubtitles: subtitles.hasOlder,
    reportError: runtime.reportError,
    clearErrorFrom: runtime.clearErrorFrom,
  });
  const captureReason = captureUnavailableReason(settings.value, runtime.capabilities.core);
  const networkReason = networkCaptureUnavailableReason(settings.value, runtime.capabilities.core);
  const networkCaptureVisible = networkCaptureFeatureEnabled(settings.value)
    && runtime.capabilities.core?.chatbox.network_receive.available === true;
  const captureControl = useCaptureControl({
    coreReady: runtime.ready,
    unavailableReason: captureReason,
    networkUnavailableReason: networkReason,
    getRunning: captureRuntime.getRunning,
    getNetworkRunning: captureRuntime.getNetworkRunning,
    outputMode: settings.value?.audio.output.mode,
    networkCaptureEnabled: usesVrchatNetwork(settings.value),
    preparePlayerAudioStart: captureRuntime.playerAudioStart.prepare,
    createConversation: conversation.createConversation,
    clearLookup: selection.clear,
    setCoreCaptureRunning: captureRuntime.setRunning,
    setCoreNetworkRunning: captureRuntime.setNetworkRunning,
    clearErrorFrom: runtime.clearErrorFrom,
    reportError: runtime.reportError,
  });
  const selectionPanelOpen = Boolean(selection.target && selection.tool);
  const overlayOpen = selectionPanelOpen || captureControl.vrchatWarningOpen || captureRuntime.playerAudioStart.notice !== null
    || captureRuntime.playerAudioStart.memory.dialog !== null;
  const resumedPlayerAudio = useRef(false);
  useEffect(() => {
    const resumeAction = captureRuntime.playerAudioStart.resumeAction;
    if (!resumeAction || resumedPlayerAudio.current || !runtime.ready || !settings.value || onboardingFlow.status !== "complete") return;
    resumedPlayerAudio.current = true;
    if (resumeAction === "start_network_capture") {
      if (networkCaptureFeatureEnabled(settings.value)) void captureControl.setNetworkCaptureRunning(true);
    } else if (usesVrchatNetwork(settings.value)) {
      void captureControl.setCaptureRunning(true);
    }
  }, [captureRuntime.playerAudioStart.resumeAction, runtime.ready, settings.value, onboardingFlow.status, captureControl.setCaptureRunning, captureControl.setNetworkCaptureRunning]);
  const { interfaceScale, setInterfaceScale } = useInterfaceScale(compactWindow.compact);
  const activeLanguageSession = runtime.health?.language_session;
  const chatboxRoute = (activeLanguageSession && activeLanguageSession.kind !== "global"
    ? activeLanguageSession.translation
    : settings.value?.translation)?.microphone_targets[0];
  const providerCatalog = useApiProfileViews(
    `${settings.value?.asr.active_profile_id ?? "local"}:${settings.value?.asr.backend ?? ""}:${chatboxRoute?.profile_id ?? ""}`,
    runtime.ready,
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
    || (
      conversation.selectedConversation
      && conversation.selectedConversation.id !== subtitles.openedConversationId
    ),
  );
  const compactSubtitles = useMemo(() => (
    compactWindow.compact
      ? subtitlesForCompactView(subtitles.items, selection.target?.context)
      : EMPTY_COMPACT_SUBTITLES
  ), [compactWindow.compact, subtitles.items, selection.target?.context]);

  const openSelectionAiSettings = useCallback(async () => {
    if (compactWindow.compact && !(await compactWindow.exitCompact())) return;
    selection.clear();
    setSettingsInitialCategory("api");
    setPage("settings");
  }, [compactWindow.compact, compactWindow.exitCompact, selection.clear, setPage, setSettingsInitialCategory]);

  const finishOnboarding = useCallback(async (startCapture: boolean) => {
    await onboardingFlow.finish({
      startCapture,
      startCaptureAction: captureControl.toggleCapture,
      startCaptureError: t("onboarding.errors.startCapture"),
    });
    setPage("live");
  }, [captureControl.toggleCapture, onboardingFlow.finish, setPage, t]);

  const locale = i18n.resolvedLanguage ?? "en-US";
  const onboarding = useMemo(() => ({
    ...onboardingFlow,
    finish: finishOnboarding,
  }), [finishOnboarding, onboardingFlow]);
  const capture = useMemo(() => ({
    ...captureRuntime,
    ...captureControl,
    available: captureReason === null,
    unavailableReason: captureReason,
    networkCaptureVisible,
    networkAvailable: networkReason === null,
    networkUnavailableReason: networkReason,
  }), [captureControl, captureReason, captureRuntime, networkCaptureVisible, networkReason]);
  const compact = useMemo(() => ({
    ...compactWindow,
    selectionPanelOpen,
    overlayOpen,
    subtitles: compactSubtitles,
    subtitleHistory: subtitles.items,
  }), [compactSubtitles, compactWindow, overlayOpen, selectionPanelOpen, subtitles.items]);
  const conversationsGroup = useMemo(() => ({
    ...conversation,
    selectedLoading: selectedConversationLoading,
  }), [conversation, selectedConversationLoading]);
  const subtitlesGroup = useMemo(() => ({
    ...subtitles,
    translationHandler,
  }), [subtitles, translationHandler]);
  const learning = useMemo(() => ({
    workspace: learningWorkspace,
    actions: learningActions,
  }), [learningActions, learningWorkspace]);
  const selectionGroup = useMemo(() => ({
    ...selection,
    openAiSettings: openSelectionAiSettings,
  }), [openSelectionAiSettings, selection]);
  const settingsGroup = useMemo(() => ({
    ...settings,
    interfaceScale,
    setInterfaceScale,
  }), [interfaceScale, setInterfaceScale, settings]);
  const translationLanguageCodes = useMemo(
    () => translationLanguageCodesForProfile(translationProfile, chatboxRoute?.backend),
    [chatboxRoute?.backend, translationProfile],
  );
  const allowCustomTranslationLanguage = supportsCustomTranslationLanguage(translationProfile);
  const providers = useMemo(() => ({
    catalog: providerCatalog,
    translationProfile,
    translationLanguageCodes,
    allowCustomTranslationLanguage,
  }), [allowCustomTranslationLanguage, providerCatalog, translationLanguageCodes, translationProfile]);
  const integrations = useMemo(() => ({ updater }), [updater]);

  return useMemo(() => ({
    locale,
    onboarding,
    runtime,
    playerDisplay,
    capture,
    compact,
    conversations: conversationsGroup,
    subtitles: subtitlesGroup,
    learning,
    selection: selectionGroup,
    settings: settingsGroup,
    providers,
    chatbox,
    integrations,
    dictionary,
  }), [
    capture,
    chatbox,
    compact,
    conversationsGroup,
    dictionary,
    integrations,
    learning,
    locale,
    onboarding,
    playerDisplay,
    providers,
    runtime,
    selectionGroup,
    settingsGroup,
    subtitlesGroup,
  ]);
}

export type AppWorkspace = ReturnType<typeof useAppWorkspace>;
