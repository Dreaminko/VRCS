import { useCallback, useEffect, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, Clock3, X } from "lucide-react";

import { conversationTime } from "./app-utils";
import type { Page } from "./app-types";
import { subtitleForCompactView } from "./compact-mode";
import { shouldShowVrchatNotRunningWarning } from "./capture-warning";
import { BottomDock } from "./components/BottomDock";
import { ChatboxComposer } from "./components/ChatboxComposer";
import { CompactView } from "./components/CompactView";
import { ConversationSidebar } from "./components/ConversationSidebar";
import { DictionaryPopover } from "./components/DictionaryPopover";
import { HistoryView, LiveView, TopStatus } from "./components/SubtitleViews";
import {
  CudaRuntimeDialog,
  VrchatNotRunningDialog,
} from "./components/WarningDialogs";
import { WindowChrome } from "./components/WindowChrome";
import { OnboardingWizard } from "./onboarding/OnboardingWizard";
import { useCompactWindow } from "./hooks/useCompactWindow";
import { useChatboxWorkspace } from "./hooks/useChatboxWorkspace";
import { useConversationWorkspace } from "./hooks/useConversationWorkspace";
import { useCoreSession } from "./hooks/useCoreSession";
import { useDictionaryLookup } from "./hooks/useDictionaryLookup";
import {
  applyInterfaceScale,
  interfaceScaleShortcutStep,
  normalizeInterfaceScale,
  readInterfaceScale,
  syncInterfaceViewportProperties,
  writeInterfaceScale,
} from "./interface-scale";
import { SettingsPanel } from "./settings/SettingsPanel";
import {
  completeOnboarding,
  loadOnboardingState,
  needsOnboarding,
  saveOnboardingProgress,
} from "./onboarding-state";
import {
  readTranscriptionStartBehavior,
  shouldCreateConversationOnCaptureToggle,
} from "./transcription-start";
import {
  supportsCustomTranslationLanguage,
  translationLanguageCodesForProvider,
} from "./translation-languages";

function App() {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const [page, setPage] = useState<Page>("live");
  const [interfaceScale, setInterfaceScale] = useState(readInterfaceScale);
  const [onboardingStatus, setOnboardingStatus] = useState<"loading" | "required" | "complete">("loading");
  const [onboardingStep, setOnboardingStep] = useState(0);
  const core = useCoreSession(page === "settings" || onboardingStatus !== "complete");
  const {
    connection,
    coreReady,
    startupFailed,
    health,
    capturePending,
    subtitles,
    partials,
    hasOlderSubtitles,
    loadingOlderSubtitles,
    loadOlderSubtitles,
    vrchatMuteStatus,
    settings,
    devices,
    devicesReady,
    asrCapabilities,
    dictionarySources,
    error,
    clearError,
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
  const [vrchatWarningOpen, setVrchatWarningOpen] = useState(false);
  const [cudaRuntimeWarningOpen, setCudaRuntimeWarningOpen] = useState(false);
  const [vrchatMuteToast, setVrchatMuteToast] = useState<{ messageKey: string; muted: boolean } | null>(null);
  const chatboxButtonRef = useRef<HTMLButtonElement | null>(null);
  const previousVrchatMuteRef = useRef<boolean | null | undefined>(undefined);
  const cudaRuntimeWarningShownRef = useRef(false);
  const compactWindow = useCompactWindow({ clearError, reportError });
  const muteToastSettingsReady = settings !== null;
  const muteToastEnabled = settings?.osc.mute_status_toast_enabled ?? false;
  const currentVrchatMute = vrchatMuteStatus?.muted ?? null;
  const translationProvider = settings?.asr.api_profiles.find(
    (profile) => profile.id === settings.translation.profile_id,
  )?.provider;
  const translationLanguageCodes = translationLanguageCodesForProvider(translationProvider);
  const vrchatMuteSyncEnabled = vrchatMuteStatus?.enabled ?? false;

  useEffect(() => {
    if (!muteToastSettingsReady) return;
    if (!muteToastEnabled) {
      previousVrchatMuteRef.current = currentVrchatMute;
      setVrchatMuteToast(null);
      return;
    }
    if (!vrchatMuteSyncEnabled || currentVrchatMute === null) {
      setVrchatMuteToast(null);
      return;
    }
    const previous = previousVrchatMuteRef.current;
    previousVrchatMuteRef.current = currentVrchatMute;
    if (previous === currentVrchatMute || (previous === undefined && !currentVrchatMute)) return;
    setVrchatMuteToast({
      muted: currentVrchatMute,
      messageKey: currentVrchatMute ? "settings.osc.muteToastMuted" : "settings.osc.muteToastUnmuted",
    });
    const timer = window.setTimeout(() => setVrchatMuteToast(null), 3_000);
    return () => window.clearTimeout(timer);
  }, [
    currentVrchatMute,
    muteToastEnabled,
    muteToastSettingsReady,
    vrchatMuteSyncEnabled,
  ]);

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
  const chatbox = useChatboxWorkspace(settings);
  const conversation = useConversationWorkspace({
    subtitles,
    page,
    running: health?.capture_requested ?? false,
  });
  const {
    conversations,
    activeConversation,
    selectedConversation,
    sidebarOpen,
    setSidebarOpen,
    sidebarWidth,
    setSidebarWidth,
    selectConversation,
    createConversation,
    renameConversation,
    setConversationIcon,
    resetConversationCustomization,
    liveScrollRef,
    followingLiveSubtitles,
    scrollLiveViewToBottom,
    onLiveScroll,
  } = conversation;
  const [sidebarResizing, setSidebarResizing] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void loadOnboardingState().then(
      (state) => {
        if (cancelled) return;
        setOnboardingStep(state.currentStep);
        setOnboardingStatus(needsOnboarding(state) ? "required" : "complete");
      },
      () => {
        if (!cancelled) setOnboardingStatus("required");
      },
    );
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    writeInterfaceScale(interfaceScale);
    void applyInterfaceScale(interfaceScale).catch((reason) => {
      reportError(reason, "errors.window.interfaceScale", "window");
    });
  }, [interfaceScale, reportError]);

  useEffect(() => {
    window.addEventListener("resize", syncInterfaceViewportProperties);
    return () => window.removeEventListener("resize", syncInterfaceViewportProperties);
  }, []);

  useEffect(() => {
    const handleInterfaceScaleShortcut = (event: KeyboardEvent) => {
      const step = interfaceScaleShortcutStep(event);
      if (step === 0) return;
      event.preventDefault();
      setInterfaceScale((current) => normalizeInterfaceScale(current + step));
    };
    window.addEventListener("keydown", handleInterfaceScaleShortcut);
    return () => window.removeEventListener("keydown", handleInterfaceScaleShortcut);
  }, []);

  useEffect(() => {
    const preventBrowserContextMenu = (event: MouseEvent) => event.preventDefault();
    document.addEventListener("contextmenu", preventBrowserContextMenu);
    return () => document.removeEventListener("contextmenu", preventBrowserContextMenu);
  }, []);

  useEffect(() => {
    if (onboardingStatus !== "complete") return;
    const runtimeMissing = Boolean(
      asrCapabilities
      && asrCapabilities.cuda.device_count > 0
      && !asrCapabilities.cuda.available,
    );
    if (runtimeMissing && !cudaRuntimeWarningShownRef.current) {
      cudaRuntimeWarningShownRef.current = true;
      setCudaRuntimeWarningOpen(true);
    } else if (!runtimeMissing) {
      cudaRuntimeWarningShownRef.current = false;
      setCudaRuntimeWarningOpen(false);
    }
  }, [asrCapabilities, onboardingStatus]);

  const toggleCapture = async (): Promise<boolean> => {
    if (!coreReady) return false;
    try {
      if (shouldCreateConversationOnCaptureToggle(
        health?.capture_requested ?? false,
        readTranscriptionStartBehavior(),
      )) {
        createConversation();
        clearLookup();
      }
      await toggleCoreCapture();
      return true;
    } catch (reason) {
      if (shouldShowVrchatNotRunningWarning(
        reason,
        settings?.audio.output.mode === "vrchat",
      )) {
        clearError();
        clearLookup();
        setVrchatWarningOpen(true);
        if (compact) {
          try {
            await resizeCompactWindow(true);
          } catch (resizeError) {
            reportError(resizeError, "errors.window.warningExpand", "window");
          }
        }
      } else {
        reportError(reason, "errors.operation", "capture");
      }
      return false;
    }
  };

  const closeVrchatWarning = () => {
    setVrchatWarningOpen(false);
    if (compact) collapseCompactOverlay();
  };

  const selectConversationAndCloseLookup = useCallback((id: string) => {
    selectConversation(id);
    clearLookup();
  }, [clearLookup, selectConversation]);

  const createConversationAndCloseLookup = useCallback(() => {
    createConversation();
    clearLookup();
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

  if (onboardingStatus !== "complete") {
    return (
      <div className="app-shell onboarding-shell">
        <WindowChrome />
        {onboardingStatus === "loading" || !settings ? (
          <div className="onboarding-loading" role="status">
            <span className="onboarding-loading-mark">VRCS</span>
            <p>{startupFailed ? t("errors.core.initialize") : t("common.loading")}</p>
            {startupFailed && <button className="primary-button" type="button" onClick={() => void retryCore()}>{t("common.retry")}</button>}
          </div>
        ) : (
          <OnboardingWizard
            initialStep={onboardingStep}
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
            onProgress={async (nextStep) => {
              await saveOnboardingProgress(nextStep);
              setOnboardingStep(nextStep);
            }}
            onSkip={async () => {
              await completeOnboarding();
              setOnboardingStatus("complete");
            }}
            onComplete={async (startCapture) => {
              if (startCapture && !(await toggleCapture())) {
                throw new Error(t("onboarding.errors.startCapture"));
              }
              await completeOnboarding();
              setOnboardingStatus("complete");
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
          partial={partials.microphone ?? partials.speaker}
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
            onClose={closeCompactLookup}
          />
        )}
        {vrchatWarningOpen && (
          <VrchatNotRunningDialog onClose={closeVrchatWarning} />
        )}
        {cudaRuntimeWarningOpen && (
          <CudaRuntimeDialog
            onClose={() => setCudaRuntimeWarningOpen(false)}
          />
        )}
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
              />
            )}

            {error && (
              <div className="error-banner" role="alert">
                <span>{error}</span>
                {startupFailed && (
                  <button type="button" onClick={() => void retryCore()}>
                    {t("common.retry")}
                  </button>
                )}
                <button
                  type="button"
                  aria-label={t("common.closeError")}
                  onClick={clearError}
                >
                  <X size={18} />
                </button>
              </div>
            )}

            {page === "live" && (
              <>
                {selectedConversation
                  && activeConversation
                  && selectedConversation.id !== activeConversation.id
                  && (
                    <div className="conversation-history-notice">
                      <Clock3 size={15} />
                      <span>
                        {t("conversations.viewingPast", {
                          time: conversationTime(
                            selectedConversation.startedAt,
                            locale,
                            t("date.today"),
                            t("date.yesterday"),
                          ),
                        })}
                      </span>
                      <button
                        type="button"
                        onClick={() => selectConversation(activeConversation.id)}
                      >
                        {t("conversations.returnCurrent")}
                      </button>
                    </div>
                  )}
                <LiveView
                  subtitles={selectedConversation?.subtitles ?? []}
                  partials={selectedConversation?.id === activeConversation?.id ? partials : {}}
                  running={
                    (health?.capture_requested ?? false)
                    && selectedConversation?.id === activeConversation?.id
                  }
                  onSelect={selectWord}
                  onTranslate={subtitleTranslationHandler}
                  translatingSubtitleIds={translatingSubtitleIds}
                />
              </>
            )}

            {page === "history" && (
              <HistoryView
                subtitles={subtitles}
                onSelect={selectWord}
                onTranslate={subtitleTranslationHandler}
                translatingSubtitleIds={translatingSubtitleIds}
                hasOlder={hasOlderSubtitles}
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
                  setOnboardingStep(0);
                  setOnboardingStatus("required");
                }}
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
          allowCustomLanguage={supportsCustomTranslationLanguage(translationProvider)}
          onDraftChange={chatbox.setDraft}
          onTranslate={chatbox.translate}
          onSend={chatbox.send}
        />
      )}

      {lookup && (
        <DictionaryPopover
          lookup={lookup}
          ankiEnabled={settings?.anki.enabled ?? true}
          onClose={clearLookup}
        />
      )}
      {vrchatWarningOpen && (
        <VrchatNotRunningDialog onClose={closeVrchatWarning} />
      )}
      {cudaRuntimeWarningOpen && (
        <CudaRuntimeDialog
          onClose={() => setCudaRuntimeWarningOpen(false)}
        />
      )}
      {vrchatMuteToast && (
        <div
          className={`vrchat-mute-toast ${vrchatMuteToast.muted ? "muted" : "ready"}`}
          role="status"
        >
          <i aria-hidden="true" />
          <span>{t(vrchatMuteToast.messageKey)}</span>
        </div>
      )}
    </div>
  );
}

export default App;
