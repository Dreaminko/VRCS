import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import type { ComponentProps, CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, LoaderCircle } from "lucide-react";

import { useHealth } from "../core-client/health-store";

import type { Page } from "./app-types";
import { prefersReducedMotion } from "./app-motion";
import { ErrorBanner } from "./components/ErrorBanner";
import { PastConversationNotice } from "./components/PastConversationNotice";
import { WorkspaceFrame } from "./components/WorkspaceFrame";
import { ChatboxComposer } from "../chatbox/ChatboxComposer";
import { ConversationSidebar } from "../conversations/ConversationSidebar";
import { useLiveConversationScroll } from "../conversations/useLiveConversationScroll";

import type { SettingsCategory } from "../settings/settings-types";
import { BottomDock } from "../shell/BottomDock";
import { WindowChrome } from "../shell/WindowChrome";
import { LiveView, TopStatus } from "../subtitles/components/SubtitleViews";
import { UpdateDetailsDialog } from "../updates/UpdateDetailsDialog";
import { UpdateNotice } from "../updates/UpdateNotice";
import { SelectionToolOverlays } from "./SelectionToolOverlays";
import type { AppWorkspace } from "./useAppWorkspace";
import { openDesktopVision } from "../vision/api";
import { PlayerHistoryControls } from "../subtitles/components/PlayerHistoryControls";
import { applyPlayerProximity, applyPlayerSourceAction, applyPlayerSourceMode, applyPlayerViewPriority, sourceMode } from "../subtitles/player-controls";

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

function LiveSettingsPanel(props: ComponentProps<typeof SettingsPanel>) {
  const liveHealth = useHealth();
  return <SettingsPanel {...props} health={liveHealth ?? props.health} />;
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
  hidden = false,
  page,
  settingsInitialCategory,
  setPage,
  setSettingsInitialCategory,
  workspace,
}: {
  hidden?: boolean;
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
    playerDisplay,
  } = workspace;
  const liveScroll = useLiveConversationScroll({
    page,
    activeConversationId: conversations.activeConversation?.id ?? null,
    selectedConversationId: conversations.selectedConversation?.id ?? null,
    openedConversationId: subtitles.openedConversationId,
    loadingConversationSubtitles: subtitles.loadingConversation,
    selectedConversationUpdatedAt: conversations.selectedConversation?.updatedAt ?? null,
    focusedSubtitleId: subtitles.focusedSubtitleId,
  });
  const features = settings.value?.features;
  const titleConfig = settings.value?.conversation_titles;
  const titleConfigured = Boolean(titleConfig?.model.trim()
    && providers.catalog.profiles.some((profile) => profile.id === titleConfig.profile_id
      && profile.capabilities.supports_text_generation));
  const [sidebarResizing, setSidebarResizing] = useState(false);
  const [visionOpenError, setVisionOpenError] = useState(false);
  const openVision = useCallback(() => {
    setVisionOpenError(false);
    void openDesktopVision().catch(() => setVisionOpenError(true));
  }, []);
  const chatboxButtonRef = useRef<HTMLButtonElement | null>(null);
  const shellRef = useRef<HTMLDivElement>(null);
  const previewSidebarWidth = useCallback((width: number) => {
    shellRef.current?.style.setProperty("--conversation-sidebar-width", `${width}px`);
  }, []);
  const openTitleSettings = useCallback(() => {
    setSettingsInitialCategory("api");
    setPage("settings");
  }, [setPage, setSettingsInitialCategory]);
  const handlePlayerSource = useCallback(
    (action: Parameters<typeof applyPlayerSourceAction>[1], speaker: Parameters<typeof applyPlayerSourceAction>[2]) =>
      settings.update(current => applyPlayerSourceAction(current, action, speaker)),
    [settings.update],
  );
  const handlePlayerSourceMode = useCallback(
    (mode: Parameters<typeof applyPlayerSourceMode>[1]) =>
      settings.update(current => applyPlayerSourceMode(current, mode)),
    [settings.update],
  );
  const handleProximityChange = useCallback(
    (radius_m: number) => settings.update(current => applyPlayerProximity(current, radius_m)),
    [settings.update],
  );
  const handleViewPriorityChange = useCallback(
    (enabled: boolean, horizontal_fov_deg: number) =>
      settings.update(current => applyPlayerViewPriority(current, enabled, horizontal_fov_deg)),
    [settings.update],
  );
  const handleClearSettingsError = useCallback(
    () => runtime.clearErrorFrom("settings"),
    [runtime.clearErrorFrom],
  );
  const handleStartOnboarding = useCallback(() => {
    if (runtime.health?.capture_requested) return;
    onboarding.restart();
  }, [onboarding.restart, runtime.health?.capture_requested]);

  useEffect(() => {
    if (hidden) return;
    const preloadDeferredPages = () => {
      if (features?.learning) preloadPage("learning");
      preloadPage("settings");
    };
    if ("requestIdleCallback" in window) {
      const idleCallback = window.requestIdleCallback(preloadDeferredPages, { timeout: 1500 });
      return () => window.cancelIdleCallback(idleCallback);
    }
    const timer = globalThis.setTimeout(preloadDeferredPages, 250);
    return () => globalThis.clearTimeout(timer);
  }, [features?.learning, hidden]);

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
  const chatboxVisible = !hidden && page === "live" && chatbox.open;
  const [chatboxRetained, setChatboxRetained] = useState(false);
  const chatboxClosing = !chatboxVisible && chatboxRetained;
  const chatboxMounted = !hidden && (chatboxVisible || (page === "live" && chatboxRetained));
  const finishChatboxClose = useCallback(() => setChatboxRetained(false), []);
  useEffect(() => {
    if (chatboxVisible) {
      setChatboxRetained(true);
      return;
    }
    if (hidden || page !== "live" || prefersReducedMotion()) {
      setChatboxRetained(false);
      return;
    }
    if (!chatboxRetained) return;
    const timer = window.setTimeout(finishChatboxClose, 180);
    return () => window.clearTimeout(timer);
  }, [chatboxVisible, finishChatboxClose, hidden, page, chatboxRetained]);

  return (
    <div
      ref={shellRef}
      hidden={hidden}
      inert={hidden}
      aria-hidden={hidden || undefined}
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
      {!hidden && <WindowChrome />}
      <div className="app-body">
        {page === "live" && (
          <ConversationSidebar
            open={conversations.sidebarOpen}
            conversations={conversations.conversations}
            activeId={conversations.activeConversation?.id}
            selectedId={conversations.selectedConversation?.id}
            width={conversations.sidebarWidth}
            onPreviewWidth={previewSidebarWidth}
            onWidthChange={conversations.setSidebarWidth}
            onResizeStateChange={setSidebarResizing}
            onToggle={toggleConversationSidebar}
            onNew={createConversation}
            onSelect={selectConversation}
            onRename={conversations.renameConversation}
            onIconChange={conversations.setConversationIcon}
            onResetCustomization={conversations.resetConversationCustomization}
            onDelete={conversations.deleteConversation}
            onGenerateTitle={conversations.generateTitle}
            titleConfigured={titleConfigured}
            onOpenTitleSettings={openTitleSettings}
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
        <WorkspaceFrame
          page={page}
          scrollRef={liveScroll.liveScrollRef}
          onScroll={liveScroll.onLiveScroll}
          toolbar={
            <TopStatus
              connection={runtime.connection}
              health={runtime.health}
              settings={settings.value}
              apiProfiles={providers.catalog.profiles}
              providerDefinitions={providers.catalog.providerDefinitions}
              controls={
                <PlayerHistoryControls
                  subtitles={conversations.selectedSubtitles}
                  useLiveHealth
                  enabled={settings.value?.audio.output.mode === "vrchat_players" && runtime.capabilities.core?.audio.player_audio.available === true}
                  proximityEnabled={
                    (settings.value?.audio.output.mode === "vrchat_players" && runtime.capabilities.core?.audio.player_audio.available === true)
                    || (settings.value?.audio.output.player_audio?.enabled === true
                      && settings.value?.vrchat_chatbox?.enabled === true
                      && runtime.capabilities.core?.chatbox.network_receive.available === true)
                  }
                  selectedUserNames={settings.value?.audio.output.player_audio?.selected_user_names}
                  sourceOverrides={settings.value?.audio.output.player_audio?.source_overrides}
                  sourceMode={sourceMode(settings.value?.audio.output.player_audio)}
                  proximity={settings.value?.audio.output.player_audio?.proximity}
                  viewPriority={settings.value?.audio.output.player_audio?.view_priority ?? { enabled: false, horizontal_fov_deg: 120 }}
                  defaultLanguages={playerDisplay.defaultLanguages}
                  preferences={playerDisplay.preferences}
                  onPreferences={playerDisplay.update}
                  onSource={handlePlayerSource}
                  onSourceMode={handlePlayerSourceMode}
                  onProximityChange={handleProximityChange}
                  onViewPriorityChange={handleViewPriorityChange}
                />}
            />
          }
        >

            {visionOpenError && <ErrorBanner error={t("visionWindow.errors.openFailed")} retryable={false}
              onRetry={openVision} onClose={() => setVisionOpenError(false)} />}
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
                  playerName={settings.value?.vrchat_player_id?.trim() || undefined}
                  hiddenTracks={playerDisplay.hiddenTracks}
                  hiddenSourceLanguages={playerDisplay.hiddenSourceLanguages}
                  scrollContainerRef={liveScroll.liveScrollRef}
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
                  onAddLearning={features?.learning ? learning.workspace.collectSubtitle : undefined}
                  onOpenLearning={features?.learning ? learning.actions.openSubtitleLearning : undefined}
                  onAnalyzeSentence={features?.learning ? learning.actions.analyzeSubtitleSentence : undefined}
                  onAddLearningSelection={features?.learning ? learning.actions.collectSubtitleSelection : undefined}
                  onOpenLearningSelection={features?.learning ? learning.actions.openSubtitleSelectionLearning : undefined}
                  onAnalyzeSelection={features?.learning ? learning.actions.analyzeSubtitleSelection : undefined}
                  onOpenLearningItem={features?.learning ? learning.actions.openLearningItem : undefined}
                  isLearningBusy={learning.actions.isSubtitleLearningBusy}
                  isLearningCaptured={learning.actions.isSubtitleLearningCaptured}
                  isLearningSelectionBusy={learning.actions.isSubtitleSelectionLearningBusy}
                  isLearningSelectionCaptured={learning.actions.isSubtitleSelectionLearningCaptured}
                  translatingSubtitleIds={subtitles.translatingIds}
                  focusedSubtitleId={subtitles.focusedSubtitleId}
                />
              </>
            )}

            {page === "learning" && features?.learning && (
              <Suspense fallback={<PageLoading />}>
                <LearningWorkspace
                  conversation={conversations.selectedConversation}
                  conversations={conversations.conversations}
                  activeConversationId={conversations.activeConversation?.id}
                  onSelectConversation={selectConversation}
                  subtitles={conversations.selectedSubtitles}
                  workspace={learning.workspace}
                  ankiAvailable={features?.anki ?? false}
                  ankiEnabled={Boolean(features?.anki && settings.value?.anki.enabled)}
                  onOpenAiSettings={openTitleSettings}
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
                <LiveSettingsPanel
                  initialCategory={settingsInitialCategory}
                  settings={settings.value}
                  playerAudioStart={capture.playerAudioStart}
                  interfaceScale={settings.interfaceScale}
                  devices={settings.devices.items}
                  devicesReady={settings.devices.ready}
                  devicesError={settings.devices.error}
                  onStartMicrophoneTest={capture.startMicrophoneTest}
                  onStopMicrophoneTest={capture.stopMicrophoneTest}
                  dictionaries={dictionary.sources}
                  disabled={runtime.health?.capture_requested ?? false}
                  health={runtime.health}
                  modelStatus={runtime.health?.asr_status ?? "unknown"}
                  onRefresh={settings.devices.refresh}
                  onRefreshSettings={settings.refresh}
                  onImportDictionary={dictionary.importFile}
                  onDeleteDictionary={dictionary.deleteById}
                  onModelsChanged={settings.asr.refresh}
                  onInterfaceScaleChange={settings.setInterfaceScale}
                  onSave={settings.save}
                  onClearSettingsError={handleClearSettingsError}
                  onTestOsc={settings.testOsc}
                  onStartOnboarding={handleStartOnboarding}
                  updater={integrations.updater}
                />
              </Suspense>
            )}
        </WorkspaceFrame>
      </div>

      {page === "live" && !liveScroll.followingLiveSubtitles && !chatbox.open && (
        <button
          className="live-scroll-to-bottom"
          type="button"
          aria-label={t(subtitles.focusedSubtitleId !== null
            ? "live.returnToLatest"
            : "live.returnToBottom")}
          data-tooltip={t(subtitles.focusedSubtitleId !== null
            ? "live.returnToLatestShort"
            : "live.returnToBottomShort")}
          onClick={() => {
            if (subtitles.focusedSubtitleId !== null && conversations.selectedConversation) {
              void subtitles.openConversation(conversations.selectedConversation.id);
            } else {
              liveScroll.scrollLiveViewToBottom();
            }
          }}
        >
          <ChevronDown size={20} strokeWidth={2} />
        </button>
      )}

      <BottomDock
        learningAvailable={features?.learning ?? false}
        chatboxAvailable={settings.value !== null}
        onVision={features?.visual_question_answering && settings.value?.vision.enabled && settings.value.vision.desktop_enabled ? openVision : undefined}
        visionDisabled={!runtime.ready || !runtime.capabilities.desktop?.screenshot.available}
        page={page}
        running={runtime.health?.capture_requested ?? false}
        captureDisabled={!runtime.ready || capture.pending || (!runtime.health?.capture_requested && !capture.available)}
        captureReason={capture.unavailableReason}
        networkCaptureVisible={capture.networkCaptureVisible}
        networkCaptureRunning={runtime.health?.network_capture_requested ?? false}
        networkCaptureDisabled={!runtime.ready || capture.networkPending || (!runtime.health?.network_capture_requested && !capture.networkAvailable)}
        chatboxOpen={page === "live" && chatbox.open}
        chatboxDisabled={!runtime.ready || settings.value === null}
        chatboxButtonRef={chatboxButtonRef}
        onPageChange={(next) => {
          selection.clear();
          if (next === "settings") setSettingsInitialCategory("system");
          setPage(next);
        }}
        onPageIntent={preloadPage}
        updateAvailable={integrations.updater.hasUnreadUpdate}
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
        onNetworkCapture={() => void capture.toggleNetworkCapture()}
      />

      {chatboxMounted && (
        <ChatboxComposer
          draft={chatbox.draft}
          preferredTarget={chatbox.preferredTarget}
          preview={chatbox.preview}
          busy={chatbox.busy}
          feedback={chatbox.feedback}
          translationStale={chatbox.translationStale}
          oscEnabled={settings.value?.osc.enabled ?? false}
          unitextEnabled={settings.value?.unitext?.enabled ?? false}
          languageCodes={providers.translationLanguageCodes}
          allowCustomLanguage={providers.allowCustomTranslationLanguage}
          closing={chatboxClosing}
          onCloseComplete={finishChatboxClose}
          onDraftChange={chatbox.setDraft}
          onTranslate={chatbox.translate}
          onSend={chatbox.send}
          onClose={closeChatbox}
        />
      )}

      {!hidden && (
        <>
          <SelectionToolOverlays
            selection={selection}
            learning={learning}
            ankiEnabled={Boolean(features?.anki && settings.value?.anki.enabled)}
          />
          <UpdateNotice
            updater={integrations.updater}
            transcriptionRunning={runtime.health?.capture_requested ?? false}
            hideError={page === "settings"}
          />
          <UpdateDetailsDialog
            updater={integrations.updater}
            transcriptionRunning={runtime.health?.capture_requested ?? false}
          />
        </>
      )}
    </div>
  );
}
