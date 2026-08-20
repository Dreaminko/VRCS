import { useCallback, useEffect, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown } from "lucide-react";

import type { Page } from "./app-types";
import { ErrorBanner } from "./components/ErrorBanner";
import { PastConversationNotice } from "./components/PastConversationNotice";
import {
  RuntimeWarningDialogs,
  VrchatMuteToast,
} from "./components/RuntimeNotices";
import { subtitleForCompactView } from "../compact-mode";

import { BottomDock } from "../shell/BottomDock";
import { ChatboxComposer } from "../chatbox/ChatboxComposer";
import { CompactView } from "../shell/CompactView";
import { ConversationSidebar } from "../conversations/ConversationSidebar";
import { DictionaryPopover } from "../dictionary/DictionaryPopover";
import { LearningWorkspace } from "../learning/components/LearningWorkspace";
import { LiveView, TopStatus } from "../subtitles/components/SubtitleViews";
import { WindowChrome } from "../shell/WindowChrome";
import { OnboardingWizard } from "../onboarding/OnboardingWizard";
import { useCompactWindow } from "../shell/useCompactWindow";
import { useChatboxWorkspace } from "../chatbox/useChatboxWorkspace";
import { useConversationWorkspace } from "../conversations/useConversationWorkspace";
import { useCaptureControl } from "../core-client/useCaptureControl";
import { useCoreSession } from "../core-client/useCoreSession";
import { useCudaRuntimeWarning } from "../core-client/useCudaRuntimeWarning";
import { useDictionaryLookup } from "../dictionary/useDictionaryLookup";
import { useLearningWorkspace } from "../learning/hooks/useLearningWorkspace";
import { useInterfaceScale } from "./useInterfaceScale";
import { useVrchatMuteToast } from "./useVrchatMuteToast";
import { SettingsPanel } from "../settings/SettingsPanel";
import { useApiProfileViews } from "../settings/hooks/useApiProfileViews";
import { useOnboardingFlow } from "../onboarding/useOnboardingFlow";
import { useSubtitleLearningActions } from "../learning/hooks/useSubtitleLearningActions";
import {
  supportsCustomTranslationLanguage,
  translationLanguageCodesForProfile,
} from "../translation-languages";
import { UpdateNotice } from "../updates/UpdateNotice";
import { useAppUpdater } from "../updates/useAppUpdater";


function App() {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const [page, setPage] = useState<Page>("live");
  const onboarding = useOnboardingFlow();
  const updater = useAppUpdater(onboarding.status === "complete");
  const core = useCoreSession(page === "settings" || onboarding.status !== "complete");
  const {
    connection,
    coreReady,
    startupFailed,
    health,
    capturePending,
    openedConversationId,
    subtitles,
    hasOlderSubtitles,
    loadingConversationSubtitles,
    loadingOlderSubtitles,
    conversationCatalogEvent,
    openConversation,
    loadOlderSubtitles,
    vrchatMuteStatus,
    settings,
    captureLanguageInput,
    setCaptureLanguageInput,
    devices,
    devicesReady,
    asrCapabilities,
    dictionarySources,
    error,
    clearError,
    clearErrorFrom,
    reportError,
    retryCore,
    loadSettings,
    loadDevices,
    loadAsrCapabilities,
    toggleCapture: toggleCoreCapture,
    startMicrophoneTest,
    stopMicrophoneTest,
    testOsc,
    saveSettings,
    importDictionary,
    deleteDictionary,
    translateSubtitle,
    translatingSubtitleIds,
  } = core;
  const chatboxButtonRef = useRef<HTMLButtonElement | null>(null);
  const { interfaceScale, setInterfaceScale } = useInterfaceScale(reportError);
  const cudaRuntimeWarning = useCudaRuntimeWarning(
    asrCapabilities,
    onboarding.status === "complete",
  );
  const vrchatMuteToast = useVrchatMuteToast({
    settingsReady: settings !== null,
    enabled: settings?.osc.mute_status_toast_enabled ?? false,
    status: vrchatMuteStatus,
  });
  const compactWindow = useCompactWindow({ clearError, reportError });
  const chatboxRoute = settings?.translation.microphone_targets[0];
  const apiProfileCatalog = useApiProfileViews(
    `${settings?.asr.active_profile_id ?? "local"}:${settings?.asr.backend ?? ""}:${chatboxRoute?.profile_id ?? ""}`,
  );
  const translationProfile = apiProfileCatalog.profiles.find(
    (profile) => profile.id === chatboxRoute?.profile_id,
  );
  const translationLanguageCodes = translationLanguageCodesForProfile(translationProfile);


  const {
    compact,
    resizeCompactWindow,
    collapseCompactOverlay,
    toggleCompact,
    closeWindow,
  } = compactWindow;
  const dictionaryLookup = useDictionaryLookup({
    enabled: settings?.dictionary.selection_lookup_enabled ?? true,
    compact,
    resizeCompactWindow,
    reportError,
  });
  const {
    lookup,
    clearLookup,
    closeCompactLookup,
    selectWord,
  } = dictionaryLookup;
  const learningWorkspace = useLearningWorkspace(page === "learning" && coreReady, coreReady);
  const openLearningPage = useCallback(() => setPage("learning"), []);
  const {
    analyzeSubtitleSelection,
    analyzeSubtitleSentence,
    collectSubtitleSelection,
    isSubtitleLearningBusy,
    isSubtitleLearningCaptured,
    isSubtitleSelectionLearningBusy,
    isSubtitleSelectionLearningCaptured,
    openLearningItem,
    openSubtitleLearning,
    openSubtitleSelectionLearning,
  } = useSubtitleLearningActions({
    workspace: learningWorkspace,
    clearLookup,
    openLearningPage,
  });
  const chatbox = useChatboxWorkspace(settings);
  const conversation = useConversationWorkspace({
    coreReady,
    openedConversationId,
    subtitles,
    conversationCatalogEvent,
    openConversation,
    page,
    running: health?.capture_requested ?? false,
    hasOlderSubtitles,
    loadingConversationSubtitles,
    reportError,
    clearErrorFrom,
  });
  const {
    conversations,
    activeConversation,
    selectedConversation,
    selectedSubtitles,
    sidebarOpen,
    setSidebarOpen,
    sidebarWidth,
    setSidebarWidth,
    selectConversation,
    createConversation,
    renameConversation,
    setConversationIcon,
    resetConversationCustomization,
    deleteConversation,
    liveScrollRef,
    followingLiveSubtitles,
    selectedConversationHasOlder,
    scrollLiveViewToBottom,
    onLiveScroll,
  } = conversation;
  const selectedConversationLoading = Boolean(
    loadingConversationSubtitles
    && selectedConversation?.id === openedConversationId,
  );
  const [sidebarResizing, setSidebarResizing] = useState(false);
  const capture = useCaptureControl({
    coreReady,
    running: health?.capture_requested ?? false,
    outputMode: settings?.audio.output.mode,
    compact,
    createConversation,
    clearLookup,
    toggleCoreCapture,
    clearError,
    resizeCompactWindow,
    collapseCompactOverlay,
    reportError,
  });
  const {
    closeVrchatWarning,
    toggleCapture,
    vrchatWarningOpen,
  } = capture;

  useEffect(() => {
    const preventBrowserContextMenu = (event: MouseEvent) => event.preventDefault();
    document.addEventListener("contextmenu", preventBrowserContextMenu);
    return () => document.removeEventListener("contextmenu", preventBrowserContextMenu);
  }, []);


  const selectConversationAndCloseLookup = useCallback((id: string) => {
    selectConversation(id);
    clearLookup();
  }, [clearLookup, selectConversation]);

  const createConversationAndCloseLookup = useCallback(async () => {
    if (await createConversation()) clearLookup();
  }, [clearLookup, createConversation]);

  const toggleConversationSidebar = useCallback(() => {
    setSidebarOpen((current) => !current);
  }, [setSidebarOpen]);

  const translateVisibleSubtitle = useCallback((id: number) => {
    void translateSubtitle(id);
  }, [translateSubtitle]);
  const subtitleTranslationHandler = settings?.translation.mode === "disabled"
    ? undefined
    : translateVisibleSubtitle;

  const openChatbox = () => {
    clearLookup();
    chatbox.show();
  };

  const closeChatbox = () => {
    chatbox.close();
    requestAnimationFrame(() => chatboxButtonRef.current?.focus());
  };

  const compactSubtitle = subtitleForCompactView(
    subtitles,
    lookup?.context,
  );

  if (onboarding.status !== "complete") {
    return (
      <div className="app-shell onboarding-shell">
        <WindowChrome />
        {onboarding.status === "loading" || !settings ? (
          <div className="onboarding-loading" role="status" data-tauri-drag-region>
            <span className="onboarding-loading-mark" data-tauri-drag-region>VRCS</span>
            <p data-tauri-drag-region>{startupFailed ? t("errors.core.initialize") : t("common.loading")}</p>
            {startupFailed && <button className="primary-button" type="button" onClick={() => void retryCore()}>{t("common.retry")}</button>}
          </div>
        ) : (
          <OnboardingWizard
            initialStep={onboarding.step}
            settings={settings}
            health={health}
            devices={devices}
            devicesReady={devicesReady}
            asrCapabilities={asrCapabilities}
            modelStatus={health?.asr_status ?? "unknown"}
            onRefreshDevices={loadDevices}
            onRefreshSettings={loadSettings}
            onModelsChanged={loadAsrCapabilities}
            onStartMicrophoneTest={startMicrophoneTest}
            onStopMicrophoneTest={stopMicrophoneTest}
            onSave={saveSettings}
            onProgress={onboarding.saveProgress}
            onSkip={onboarding.skip}
            onComplete={async (startCapture) => {
              await onboarding.finish({
                startCapture,
                startCaptureAction: toggleCapture,
                startCaptureError: t("onboarding.errors.startCapture"),
              });
              setPage("live");
            }}
          />
        )}
      </div>
    );
  }

  if (compact) {
    return (
      <div className={`compact-root ${lookup ? "compact-root-lookup" : ""}`}>
        <CompactView
          subtitle={compactSubtitle}
          running={health?.capture_requested ?? false}
          vrchatMuted={vrchatMuteStatus?.muted === true}
          captureDisabled={!coreReady || capturePending}
          onSelect={selectWord}
          onCapture={() => void toggleCapture()}
          onRestore={() => void toggleCompact(clearLookup)}
          onClose={() => void closeWindow()}
        />
        {lookup && (
          <DictionaryPopover
            lookup={lookup}
            ankiEnabled={settings?.anki.enabled ?? true}
            compact
            onAddLearning={learningWorkspace.collectLookup}
            onClose={closeCompactLookup}
          />
        )}
        <RuntimeWarningDialogs
          vrchatWarningOpen={vrchatWarningOpen}
          cudaRuntimeWarningOpen={cudaRuntimeWarning.open}
          onCloseVrchatWarning={closeVrchatWarning}
          onCloseCudaRuntimeWarning={cudaRuntimeWarning.close}
        />
      </div>
    );
  }

  return (
    <div
      className={[
        "app-shell",
        page === "live" ? "live-shell" : "",
        sidebarOpen ? "sidebar-open" : "sidebar-collapsed",
        sidebarResizing ? "sidebar-resizing" : "",
      ].join(" ")}
      style={{
        "--conversation-sidebar-width": `${sidebarWidth}px`,
      } as CSSProperties}
    >
      <WindowChrome />
      <div className="app-body">
        {page === "live" && (
          <ConversationSidebar
            open={sidebarOpen}
            conversations={conversations}
            activeId={activeConversation?.id}
            selectedId={selectedConversation?.id}
            width={sidebarWidth}
            onWidthChange={setSidebarWidth}
            onResizeStateChange={setSidebarResizing}
            onToggle={toggleConversationSidebar}
            onNew={createConversationAndCloseLookup}
            onSelect={selectConversationAndCloseLookup}
            onRename={renameConversation}
            onIconChange={setConversationIcon}
            onResetCustomization={resetConversationCustomization}
            onDelete={deleteConversation}
          />
        )}
        {page === "live" && sidebarOpen && (
          <button
            className="sidebar-scrim"
            type="button"
            aria-label={t("conversations.closeSidebar")}
            onClick={() => setSidebarOpen(false)}
          />
        )}
        <div
          className="app-scroll-region"
          ref={liveScrollRef}
          onScroll={onLiveScroll}
        >
          <main className={`workspace workspace-${page}`}>
            {page === "live" && (
              <TopStatus
                connection={connection}
                health={health}
                settings={settings}
                apiProfiles={apiProfileCatalog.profiles}
                providerDefinitions={apiProfileCatalog.providerDefinitions}
                captureLanguageInput={captureLanguageInput}
                onCaptureLanguageInputChange={setCaptureLanguageInput}
              />
            )}

            {error && (
              <ErrorBanner
                error={error}
                retryable={startupFailed}
                onRetry={() => void retryCore()}
                onClose={clearError}
              />
            )}

            {page === "live" && (
              <>
                {selectedConversation
                  && activeConversation
                  && selectedConversation.id !== activeConversation.id
                  && (
                    <PastConversationNotice
                      conversation={selectedConversation}
                      locale={locale}
                      onReturnCurrent={() => selectConversation(activeConversation.id)}
                    />
                  )}
                <LiveView
                  subtitles={selectedSubtitles}
                  scrollContainerRef={liveScrollRef}
                  running={
                    (health?.capture_requested ?? false)
                    && selectedConversation?.id === activeConversation?.id
                  }
                  hasOlder={selectedConversationHasOlder}
                  loading={selectedConversationLoading}
                  loadingOlder={loadingOlderSubtitles}
                  onLoadOlder={loadOlderSubtitles}
                  onSelect={selectWord}
                  onTranslate={subtitleTranslationHandler}
                  onAddLearning={learningWorkspace.collectSubtitle}
                  onOpenLearning={openSubtitleLearning}
                  onAnalyzeSentence={analyzeSubtitleSentence}
                  onAddLearningSelection={collectSubtitleSelection}
                  onOpenLearningSelection={openSubtitleSelectionLearning}
                  onAnalyzeSelection={analyzeSubtitleSelection}
                  onOpenLearningItem={openLearningItem}
                  isLearningBusy={isSubtitleLearningBusy}
                  isLearningCaptured={isSubtitleLearningCaptured}
                  isLearningSelectionBusy={isSubtitleSelectionLearningBusy}
                  isLearningSelectionCaptured={isSubtitleSelectionLearningCaptured}
                  translatingSubtitleIds={translatingSubtitleIds}
                />
              </>
            )}

            {page === "learning" && (
              <LearningWorkspace
                conversation={selectedConversation}
                subtitles={selectedSubtitles}
                workspace={learningWorkspace}
                ankiEnabled={settings?.anki.enabled ?? true}
                onSelect={selectWord}
                onTranslate={subtitleTranslationHandler}
                translatingSubtitleIds={translatingSubtitleIds}
                hasOlder={selectedConversationHasOlder}
                loading={selectedConversationLoading}
                loadingOlder={loadingOlderSubtitles}
                onLoadOlder={loadOlderSubtitles}
              />
            )}

            {page === "settings" && settings && (
              <SettingsPanel
                settings={settings}
                interfaceScale={interfaceScale}
                devices={devices}
                devicesReady={devicesReady}
                onStartMicrophoneTest={startMicrophoneTest}
                onStopMicrophoneTest={stopMicrophoneTest}
                dictionaries={dictionarySources}
                disabled={health?.capture_requested ?? false}
                health={health}
                modelStatus={health?.asr_status ?? "unknown"}
                asrCapabilities={asrCapabilities}
                onRefresh={loadDevices}
                onRefreshSettings={loadSettings}
                onImportDictionary={importDictionary}
                onDeleteDictionary={deleteDictionary}
                onModelsChanged={loadAsrCapabilities}
                onInterfaceScaleChange={setInterfaceScale}
                onSave={saveSettings}
                onTestOsc={testOsc}
                onStartOnboarding={() => {
                  if (health?.capture_requested) return;
                  onboarding.restart();
                }}
                updater={updater}
              />
            )}
          </main>
        </div>
      </div>

      {page === "live" && !followingLiveSubtitles && !chatbox.open && (
        <button
          className="live-scroll-to-bottom"
          type="button"
          aria-label={t("live.returnToBottom")}
          title={t("live.returnToBottomShort")}
          onClick={() => scrollLiveViewToBottom()}
        >
          <ChevronDown size={20} strokeWidth={2} />
        </button>
      )}

      <BottomDock
        page={page}
        running={health?.capture_requested ?? false}
        captureDisabled={!coreReady || capturePending}
        chatboxOpen={page === "live" && chatbox.open}
        chatboxDisabled={!coreReady || settings === null}
        chatboxButtonRef={chatboxButtonRef}
        onPageChange={(next) => {
          clearLookup();
          setPage(next);
        }}
        onCompact={() => {
          void toggleCompact(clearLookup);
        }}
        onChatbox={() => {
          if (page !== "live") {
            setPage("live");
            openChatbox();
            return;
          }
          if (chatbox.open) closeChatbox();
          else openChatbox();
        }}
        onCapture={() => void toggleCapture()}
      />

      {page === "live" && chatbox.open && (
        <ChatboxComposer
          draft={chatbox.draft}
          preview={chatbox.preview}
          busy={chatbox.busy}
          feedback={chatbox.feedback}
          translationStale={chatbox.translationStale}
          oscEnabled={settings?.osc.enabled ?? false}
          languageCodes={translationLanguageCodes}
          allowCustomLanguage={supportsCustomTranslationLanguage(translationProfile)}
          onDraftChange={chatbox.setDraft}
          onTranslate={chatbox.translate}
          onSend={chatbox.send}
        />
      )}

      {lookup && (
        <DictionaryPopover
          lookup={lookup}
          ankiEnabled={settings?.anki.enabled ?? true}
          onAddLearning={learningWorkspace.collectLookup}
          onClose={clearLookup}
        />
      )}
      <RuntimeWarningDialogs
        vrchatWarningOpen={vrchatWarningOpen}
        cudaRuntimeWarningOpen={cudaRuntimeWarning.open}
        onCloseVrchatWarning={closeVrchatWarning}
        onCloseCudaRuntimeWarning={cudaRuntimeWarning.close}
      />
      <UpdateNotice
        updater={updater}
        transcriptionRunning={health?.capture_requested ?? false}
      />
      {vrchatMuteToast && (
        <VrchatMuteToast
          muted={vrchatMuteToast.muted}
          messageKey={vrchatMuteToast.messageKey}
        />
      )}
    </div>
  );
}

export default App;
