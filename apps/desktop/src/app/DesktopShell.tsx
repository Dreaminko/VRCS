import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, LoaderCircle } from "lucide-react";

import type { Page } from "./app-types";
import { ErrorBanner } from "./components/ErrorBanner";
import { PastConversationNotice } from "./components/PastConversationNotice";
import { VrchatMuteToast } from "./components/RuntimeNotices";
import { ChatboxComposer } from "../chatbox/ChatboxComposer";
import { ConversationSidebar } from "../conversations/ConversationSidebar";

import type { SettingsCategory } from "../settings/settings-types";
import { BottomDock } from "../shell/BottomDock";
import { WindowChrome } from "../shell/WindowChrome";
import { LiveView, TopStatus } from "../subtitles/components/SubtitleViews";
import { UpdateNotice } from "../updates/UpdateNotice";
import { SelectionToolOverlays } from "./SelectionToolOverlays";
import type { AppWorkspace } from "./useAppWorkspace";

let learningWorkspaceModule: Promise<typeof import("../learning/components/LearningWorkspace")> | undefined;
let settingsPanelModule: Promise<typeof import("../settings/SettingsPanel")> | undefined;

function loadLearningWorkspace() {
  learningWorkspaceModule ??= import("../learning/components/LearningWorkspace").catch((error) => {
    learningWorkspaceModule = undefined;
    throw error;
  });
  return learningWorkspaceModule;
}

function loadSettingsPanel() {
  settingsPanelModule ??= import("../settings/SettingsPanel").catch((error) => {
    settingsPanelModule = undefined;
    throw error;
  });
  return settingsPanelModule;
}

const LearningWorkspace = lazy(() => loadLearningWorkspace().then(
  ({ LearningWorkspace }) => ({ default: LearningWorkspace }),
));
const SettingsPanel = lazy(() => loadSettingsPanel().then(
  ({ SettingsPanel }) => ({ default: SettingsPanel }),
));

function preloadPage(page: Page) {
  if (page === "learning") void loadLearningWorkspace().catch(() => undefined);
  if (page === "settings") void loadSettingsPanel().catch(() => undefined);
}

function PageLoading() {
  const { t } = useTranslation();
  return (
    <div className="empty-state" role="status">
      <LoaderCircle className="spinning" size={22} />
      <p>{t("common.loading")}</p>
    </div>
  );
}

export function DesktopShell({
  page,
  settingsInitialCategory,
  setPage,
  setSettingsInitialCategory,
  workspace,
}: {
  page: Page;
  settingsInitialCategory: SettingsCategory;
  setPage: (page: Page) => void;
  setSettingsInitialCategory: (category: SettingsCategory) => void;
  workspace: AppWorkspace;
}) {
  const { t } = useTranslation();
  const {
    runtime,
    capture,
    compact,
    conversations,
    subtitles,
    learning,
    selection,
    settings,
    providers,
    chatbox,
    integrations,
    dictionary,
    onboarding,
    locale,
  } = workspace;
  const [sidebarResizing, setSidebarResizing] = useState(false);
  const chatboxButtonRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    const preloadDeferredPages = () => {
      preloadPage("learning");
      preloadPage("settings");
    };
    if ("requestIdleCallback" in window) {
      const idleCallback = window.requestIdleCallback(preloadDeferredPages, { timeout: 1500 });
      return () => window.cancelIdleCallback(idleCallback);
    }
    const timer = globalThis.setTimeout(preloadDeferredPages, 250);
    return () => globalThis.clearTimeout(timer);
  }, []);

  const selectConversation = useCallback((id: string) => {
    conversations.selectConversation(id);
    selection.clear();
  }, [conversations.selectConversation, selection.clear]);
  const selectConversationAt = useCallback((conversationId: string, subtitleId: number) => {
    conversations.selectConversationAt(conversationId, subtitleId);
    selection.clear();
  }, [conversations.selectConversationAt, selection.clear]);
  const createConversation = useCallback(async () => {
    if (await conversations.createConversation()) selection.clear();
  }, [conversations.createConversation, selection.clear]);
  const toggleConversationSidebar = useCallback(() => {
    conversations.setSidebarOpen((current) => !current);
  }, [conversations.setSidebarOpen]);
  const openChatbox = useCallback(() => {
    selection.clear();
    chatbox.show();
  }, [chatbox.show, selection.clear]);
  const closeChatbox = useCallback(() => {
    chatbox.close();
    requestAnimationFrame(() => chatboxButtonRef.current?.focus());
  }, [chatbox.close]);

  return (
    <div
      className={[
        "app-shell",
        page === "live" ? "live-shell" : "",
        conversations.sidebarOpen ? "sidebar-open" : "sidebar-collapsed",
        sidebarResizing ? "sidebar-resizing" : "",
      ].join(" ")}
      style={{
        "--conversation-sidebar-width": `${conversations.sidebarWidth}px`,
      } as CSSProperties}
    >
      <WindowChrome />
      <div className="app-body">
        {page === "live" && (
          <ConversationSidebar
            open={conversations.sidebarOpen}
            conversations={conversations.conversations}
            activeId={conversations.activeConversation?.id}
            selectedId={conversations.selectedConversation?.id}
            width={conversations.sidebarWidth}
            onWidthChange={conversations.setSidebarWidth}
            onResizeStateChange={setSidebarResizing}
            onToggle={toggleConversationSidebar}
            onNew={createConversation}
            onSelect={selectConversation}
            onRename={conversations.renameConversation}
            onIconChange={conversations.setConversationIcon}
            onResetCustomization={conversations.resetConversationCustomization}
            onDelete={conversations.deleteConversation}
            onSelectAt={selectConversationAt}
            onFocusSearch={conversations.search.focus}
            search={conversations.search}
          />
        )}
        {page === "live" && conversations.sidebarOpen && (
          <button
            className="sidebar-scrim"
            type="button"
            aria-label={t("conversations.closeSidebar")}
            onClick={() => conversations.setSidebarOpen(false)}
          />
        )}
        <div
          className="app-scroll-region"
          ref={conversations.liveScrollRef}
          onScroll={conversations.onLiveScroll}
        >
          <main className={`workspace workspace-${page}`}>
            {page === "live" && (
              <TopStatus
                connection={runtime.connection}
                health={runtime.health}
                settings={settings.value}
                apiProfiles={providers.catalog.profiles}
                providerDefinitions={providers.catalog.providerDefinitions}
              />
            )}

            {runtime.error && (
              <ErrorBanner
                error={runtime.error}
                retryable={runtime.startupFailed}
                onRetry={() => void runtime.retry()}
                onClose={runtime.clearError}
              />
            )}

            {page === "live" && (
              <>
                {conversations.selectedConversation
                  && conversations.activeConversation
                  && conversations.selectedConversation.id !== conversations.activeConversation.id
                  && (
                    <PastConversationNotice
                      conversation={conversations.selectedConversation}
                      locale={locale}
                      onReturnCurrent={() => conversations.selectConversation(
                        conversations.activeConversation!.id,
                      )}
                    />
                  )}
                <LiveView
                  subtitles={conversations.selectedSubtitles}
                  scrollContainerRef={conversations.liveScrollRef}
                  running={
                    (runtime.health?.capture_requested ?? false)
                    && conversations.selectedConversation?.id === conversations.activeConversation?.id
                    && subtitles.focusedSubtitleId === null
                  }
                  hasOlder={conversations.selectedConversationHasOlder}
                  loading={conversations.selectedLoading}
                  loadingOlder={subtitles.loadingOlder}
                  onLoadOlder={subtitles.loadOlder}
                  onSelect={selection.selectText}
                  onTranslate={subtitles.translationHandler}
                  onAddLearning={learning.workspace.collectSubtitle}
                  onOpenLearning={learning.actions.openSubtitleLearning}
                  onAnalyzeSentence={learning.actions.analyzeSubtitleSentence}
                  onAddLearningSelection={learning.actions.collectSubtitleSelection}
                  onOpenLearningSelection={learning.actions.openSubtitleSelectionLearning}
                  onAnalyzeSelection={learning.actions.analyzeSubtitleSelection}
                  onOpenLearningItem={learning.actions.openLearningItem}
                  isLearningBusy={learning.actions.isSubtitleLearningBusy}
                  isLearningCaptured={learning.actions.isSubtitleLearningCaptured}
                  isLearningSelectionBusy={learning.actions.isSubtitleSelectionLearningBusy}
                  isLearningSelectionCaptured={learning.actions.isSubtitleSelectionLearningCaptured}
                  translatingSubtitleIds={subtitles.translatingIds}
                  focusedSubtitleId={subtitles.focusedSubtitleId}
                />
              </>
            )}

            {page === "learning" && (
              <Suspense fallback={<PageLoading />}>
                <LearningWorkspace
                  conversation={conversations.selectedConversation}
                  subtitles={conversations.selectedSubtitles}
                  workspace={learning.workspace}
                  ankiEnabled={settings.value?.anki.enabled ?? true}
                  onSelect={selection.selectText}
                  onTranslate={subtitles.translationHandler}
                  translatingSubtitleIds={subtitles.translatingIds}
                  hasOlder={conversations.selectedConversationHasOlder}
                  loading={conversations.selectedLoading}
                  loadingOlder={subtitles.loadingOlder}
                  onLoadOlder={subtitles.loadOlder}
                />
              </Suspense>
            )}

            {page === "settings" && settings.value && (
              <Suspense fallback={<PageLoading />}>
                <SettingsPanel
                  initialCategory={settingsInitialCategory}
                  settings={settings.value}
                  interfaceScale={settings.interfaceScale}
                  devices={settings.devices.items}
                  devicesReady={settings.devices.ready}
                  onStartMicrophoneTest={capture.startMicrophoneTest}
                  onStopMicrophoneTest={capture.stopMicrophoneTest}
                  dictionaries={dictionary.sources}
                  disabled={runtime.health?.capture_requested ?? false}
                  health={runtime.health}
                  modelStatus={runtime.health?.asr_status ?? "unknown"}
                  asrCapabilities={settings.asr.capabilities}
                  onRefresh={settings.devices.refresh}
                  onRefreshSettings={settings.refresh}
                  onImportDictionary={dictionary.importFile}
                  onDeleteDictionary={dictionary.deleteById}
                  onModelsChanged={settings.asr.refresh}
                  onInterfaceScaleChange={settings.setInterfaceScale}
                  onSave={settings.save}
                  onTestOsc={settings.testOsc}
                  onStartOnboarding={() => {
                    if (runtime.health?.capture_requested) return;
                    onboarding.restart();
                  }}
                  updater={integrations.updater}
                />
              </Suspense>
            )}
          </main>
        </div>
      </div>

      {page === "live" && !conversations.followingLiveSubtitles && !chatbox.open && (
        <button
          className="live-scroll-to-bottom"
          type="button"
          aria-label={t(subtitles.focusedSubtitleId !== null
            ? "live.returnToLatest"
            : "live.returnToBottom")}
          title={t(subtitles.focusedSubtitleId !== null
            ? "live.returnToLatestShort"
            : "live.returnToBottomShort")}
          onClick={() => {
            if (subtitles.focusedSubtitleId !== null && conversations.selectedConversation) {
              void subtitles.openConversation(conversations.selectedConversation.id);
            } else {
              conversations.scrollLiveViewToBottom();
            }
          }}
        >
          <ChevronDown size={20} strokeWidth={2} />
        </button>
      )}

      <BottomDock
        page={page}
        running={runtime.health?.capture_requested ?? false}
        captureDisabled={!runtime.ready || capture.pending}
        chatboxOpen={page === "live" && chatbox.open}
        chatboxDisabled={!runtime.ready || settings.value === null}
        chatboxButtonRef={chatboxButtonRef}
        onPageChange={(next) => {
          selection.clear();
          if (next === "settings") setSettingsInitialCategory("system");
          setPage(next);
        }}
        onPageIntent={preloadPage}
        onCompact={() => void compact.enterCompact()}
        onChatbox={() => {
          if (page !== "live") {
            setPage("live");
            openChatbox();
            return;
          }
          if (chatbox.open) closeChatbox();
          else openChatbox();
        }}
        onCapture={() => void capture.toggleCapture()}
      />

      {page === "live" && chatbox.open && (
        <ChatboxComposer
          draft={chatbox.draft}
          preview={chatbox.preview}
          busy={chatbox.busy}
          feedback={chatbox.feedback}
          translationStale={chatbox.translationStale}
          oscEnabled={settings.value?.osc.enabled ?? false}
          languageCodes={providers.translationLanguageCodes}
          allowCustomLanguage={providers.allowCustomTranslationLanguage}
          onDraftChange={chatbox.setDraft}
          onTranslate={chatbox.translate}
          onSend={chatbox.send}
        />
      )}

      <SelectionToolOverlays
        selection={selection}
        learning={learning}
        ankiEnabled={settings.value?.anki.enabled ?? true}
      />
      <UpdateNotice
        updater={integrations.updater}
        transcriptionRunning={runtime.health?.capture_requested ?? false}
      />
      {runtime.vrchatMuteToast && (
        <VrchatMuteToast
          muted={runtime.vrchatMuteToast.muted}
          messageKey={runtime.vrchatMuteToast.messageKey}
        />
      )}
    </div>
  );
}
