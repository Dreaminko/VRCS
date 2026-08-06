import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, Clock3, X } from "lucide-react";

import { conversationTime } from "./app-utils";
import type { Page } from "./app-types";
import { subtitleForCompactView } from "./compact-mode";
import { shouldShowVrchatNotRunningWarning } from "./capture-warning";
import { BottomDock } from "./components/BottomDock";
import { CompactView } from "./components/CompactView";
import { ConversationSidebar } from "./components/ConversationSidebar";
import { DictionaryPopover } from "./components/DictionaryPopover";
import { HistoryView, LiveView, TopStatus } from "./components/SubtitleViews";
import {
  CudaRuntimeDialog,
  VrchatNotRunningDialog,
} from "./components/WarningDialogs";
import { WindowChrome } from "./components/WindowChrome";
import { useCompactWindow } from "./hooks/useCompactWindow";
import { useConversationWorkspace } from "./hooks/useConversationWorkspace";
import { useCoreSession } from "./hooks/useCoreSession";
import { useDictionaryLookup } from "./hooks/useDictionaryLookup";
import {
  applyInterfaceScale,
  interfaceScaleShortcutStep,
  normalizeInterfaceScale,
  readInterfaceScale,
  writeInterfaceScale,
} from "./interface-scale";
import { SettingsPanel } from "./settings/SettingsPanel";

function App() {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const [page, setPage] = useState<Page>("live");
  const [interfaceScale, setInterfaceScale] = useState(readInterfaceScale);
  const core = useCoreSession(page === "settings");
  const {
    connection,
    coreReady,
    startupFailed,
    health,
    subtitles,
    partials,
    settings,
    devices,
    devicesReady,
    asrCapabilities,
    dictionarySources,
    error,
    clearError,
    reportError,
    retryCore,
    loadDevices,
    loadAsrCapabilities,
    toggleCapture: toggleCoreCapture,
    saveSettings,
    importDictionary,
    deleteDictionary,
  } = core;
  const [vrchatWarningOpen, setVrchatWarningOpen] = useState(false);
  const [cudaRuntimeWarningOpen, setCudaRuntimeWarningOpen] = useState(false);
  const cudaRuntimeWarningShownRef = useRef(false);
  const compactWindow = useCompactWindow({ clearError, reportError });
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
  const conversation = useConversationWorkspace({
    subtitles,
    page,
    running: health?.capture_running ?? false,
  });
  const {
    conversations,
    activeConversation,
    selectedConversation,
    sidebarOpen,
    setSidebarOpen,
    selectConversation,
    createConversation,
    liveScrollRef,
    followingLiveSubtitles,
    scrollLiveViewToBottom,
    onLiveScroll,
  } = conversation;

  useEffect(() => {
    writeInterfaceScale(interfaceScale);
    void applyInterfaceScale(interfaceScale).catch((reason) => {
      reportError(reason, "errors.window.interfaceScale");
    });
  }, [interfaceScale, reportError]);

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
  }, [asrCapabilities]);

  const toggleCapture = async () => {
    if (!coreReady) return;
    try {
      await toggleCoreCapture();
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
            reportError(resizeError, "errors.window.warningExpand");
          }
        }
      } else {
        reportError(reason, "errors.operation");
      }
    }
  };

  const closeVrchatWarning = () => {
    setVrchatWarningOpen(false);
    if (compact) collapseCompactOverlay();
  };

  const selectConversationAndCloseLookup = (id: string) => {
    selectConversation(id);
    clearLookup();
  };

  const createConversationAndCloseLookup = () => {
    createConversation();
    clearLookup();
  };

  const compactSubtitle = subtitleForCompactView(
    subtitles,
    lookup?.context,
  );

  if (compact) {
    return (
      <div className={`compact-root ${lookup ? "compact-root-lookup" : ""}`}>
        <CompactView
          subtitle={compactSubtitle}
          partial={partials.microphone ?? partials.speaker}
          running={health?.capture_running ?? false}
          captureDisabled={!coreReady}
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
      ].join(" ")}
    >
      <WindowChrome />
      <div className="app-body">
        {page === "live" && (
          <ConversationSidebar
            open={sidebarOpen}
            conversations={conversations}
            activeId={activeConversation?.id}
            selectedId={selectedConversation?.id}
            onToggle={() => setSidebarOpen((current) => !current)}
            onNew={createConversationAndCloseLookup}
            onSelect={selectConversationAndCloseLookup}
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
                  subtitles={(selectedConversation?.subtitles ?? []).slice(0, 12)}
                  partials={selectedConversation?.id === activeConversation?.id ? partials : {}}
                  running={
                    (health?.capture_running ?? false)
                    && selectedConversation?.id === activeConversation?.id
                  }
                  onSelect={selectWord}
                />
              </>
            )}

            {page === "history" && (
              <HistoryView subtitles={subtitles} onSelect={selectWord} />
            )}

            {page === "settings" && settings && (
              <SettingsPanel
                settings={settings}
                interfaceScale={interfaceScale}
                devices={devices}
                devicesReady={devicesReady}
                dictionaries={dictionarySources}
                disabled={health?.capture_running ?? false}
                modelStatus={health?.asr_status ?? "unknown"}
                asrCapabilities={asrCapabilities}
                onRefresh={loadDevices}
                onImportDictionary={importDictionary}
                onDeleteDictionary={deleteDictionary}
                onModelsChanged={loadAsrCapabilities}
                onInterfaceScaleChange={setInterfaceScale}
                onSave={saveSettings}
              />
            )}
          </main>
        </div>
      </div>

      {page === "live" && !followingLiveSubtitles && (
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
        running={health?.capture_running ?? false}
        captureDisabled={!coreReady}
        onPageChange={(next) => {
          clearLookup();
          setPage(next);
        }}
        onCompact={() => void toggleCompact(clearLookup)}
        onCapture={() => void toggleCapture()}
      />

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
    </div>
  );
}

export default App;
