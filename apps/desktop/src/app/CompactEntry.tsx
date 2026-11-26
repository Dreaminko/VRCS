import { useCallback, useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type TransitionEvent } from "react";
import { ChatboxComposer } from "../chatbox/ChatboxComposer";
import { CompactView } from "../shell/CompactView";
import { COMPACT_FONT_PRESETS, COMPACT_LAYOUT } from "../shell/compact-layout";
import { COMPACT_WINDOW_MAX_HEIGHT } from "../shell/compact-mode";
import { NATIVE_APP } from "./app-environment";
import { prefersReducedMotion } from "./app-motion";
import { SelectionToolOverlays } from "./SelectionToolOverlays";
import type { AppWorkspace } from "./useAppWorkspace";

const layoutVariables = Object.fromEntries(Object.entries(COMPACT_LAYOUT).map(([name, value]) => [
  `--compact-${name.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`)}`, `${value}px`,
]));

const REVEAL_TRANSITION_FALLBACK_MS = 260;
const SHRINK_VIEWPORT_SYNC_FALLBACK_MS = 60;

function measureTargetChatboxHeight(container: HTMLElement, targetExpanded: boolean): number {
  const detailsReveal = container.querySelector<HTMLElement>(".chatbox-details-reveal");
  const details = container.querySelector<HTMLElement>(".chatbox-details");
  const totalHeight = container.getBoundingClientRect().height;
  const currentRevealHeight = detailsReveal?.getBoundingClientRect().height ?? 0;
  let targetRevealHeight = 0;
  if (targetExpanded && details) {
    const style = getComputedStyle(details);
    targetRevealHeight = details.getBoundingClientRect().height
      + (Number.parseFloat(style.marginTop) || 0)
      + (Number.parseFloat(style.marginBottom) || 0);
  }
  return Math.ceil(Math.max(0, totalHeight - currentRevealHeight + targetRevealHeight));
}

export function CompactEntry({
  compact,
  runtime,
  capture,
  settings,
  selection,
  learning,
  playerDisplay,
  chatbox,
  providers,
}: {
  compact: AppWorkspace["compact"];
  runtime: AppWorkspace["runtime"];
  capture: AppWorkspace["capture"];
  settings: AppWorkspace["settings"];
  selection: AppWorkspace["selection"];
  learning: AppWorkspace["learning"];
  playerDisplay: AppWorkspace["playerDisplay"];
  chatbox: AppWorkspace["chatbox"];
  providers: AppWorkspace["providers"];
}) {
  const { selectionPanelOpen, overlayOpen, resizeCompactWindow } = compact;
  const { reportError } = runtime;
  const chatboxButtonRef = useRef<HTMLButtonElement>(null);
  const chatboxRef = useRef<HTMLDivElement>(null);
  const chatboxVisible = chatbox.open && !overlayOpen;
  const [chatboxMounted, setChatboxMounted] = useState(chatboxVisible);
  const [chatboxOpen, setChatboxOpen] = useState(chatboxVisible);
  const [heightLocked, setHeightLocked] = useState(chatboxVisible);
  const [expanded, setExpanded] = useState(false);
  const [chatboxHeight, setChatboxHeight] = useState(0);
  const expandedIntentRef = useRef(false);
  const animatingRef = useRef(false);
  const closingRef = useRef(false);
  const chatboxOpenRef = useRef(chatboxOpen);
  chatboxOpenRef.current = chatboxOpen;
  const compactHeightRef = useRef(compact.height);
  compactHeightRef.current = compact.height;
  const openTokenRef = useRef(0);
  const expandTokenRef = useRef(0);
  const timerRef = useRef<number | null>(null);
  const unlockCleanupRef = useRef<(() => void) | null>(null);
  const panelState = overlayOpen ? true : chatboxMounted ? chatboxHeight : false;

  const clearPendingTimer = useCallback(() => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const clearUnlockWait = useCallback(() => {
    unlockCleanupRef.current?.();
    unlockCleanupRef.current = null;
  }, []);

  const syncSteadyHeight = useCallback(() => {
    const element = chatboxRef.current;
    if (!element) return;
    const next = measureTargetChatboxHeight(element, expandedIntentRef.current);
    setChatboxHeight((current) => (current === next ? current : next));
  }, []);

  const finishClose = useCallback(() => {
    clearPendingTimer();
    clearUnlockWait();
    animatingRef.current = false;
    closingRef.current = false;
    setChatboxMounted(false);
    setChatboxOpen(false);
    if (!chatbox.open) {
      setExpanded(false);
      expandedIntentRef.current = false;
    }
    setChatboxHeight(0);
    if (overlayOpen) {
      setHeightLocked(false);
      return;
    }
    const token = ++openTokenRef.current;
    void resizeCompactWindow(false).finally(() => {
      if (openTokenRef.current !== token) return;
      const unlock = () => {
        clearUnlockWait();
        if (openTokenRef.current === token) setHeightLocked(false);
      };
      if (!NATIVE_APP || window.innerHeight <= compactHeightRef.current + 1) {
        unlock();
        return;
      }
      const onResize = () => {
        if (window.innerHeight <= compactHeightRef.current + 1) unlock();
      };
      const fallback = window.setTimeout(unlock, SHRINK_VIEWPORT_SYNC_FALLBACK_MS);
      window.addEventListener("resize", onResize);
      unlockCleanupRef.current = () => {
        window.removeEventListener("resize", onResize);
        window.clearTimeout(fallback);
      };
    });
  }, [chatbox.open, clearPendingTimer, clearUnlockWait, overlayOpen, resizeCompactWindow]);

  useLayoutEffect(() => {
    if (chatboxVisible) {
      closingRef.current = false;
      clearPendingTimer();
      clearUnlockWait();
      setHeightLocked(true);
      if (!chatboxMounted) setChatboxMounted(true);
      return;
    }
    if (!chatboxMounted) {
      if (overlayOpen && heightLocked) {
        clearUnlockWait();
        setHeightLocked(false);
      }
      return;
    }
    ++openTokenRef.current;
    ++expandTokenRef.current;
    if (overlayOpen || prefersReducedMotion() || (!chatboxOpenRef.current && !closingRef.current)) {
      finishClose();
      return;
    }
    if (closingRef.current) return;
    closingRef.current = true;
    animatingRef.current = true;
    setChatboxOpen(false);
    timerRef.current = window.setTimeout(finishClose, REVEAL_TRANSITION_FALLBACK_MS);
  }, [chatboxVisible, chatboxMounted, heightLocked, overlayOpen, clearPendingTimer, clearUnlockWait, finishClose]);

  useLayoutEffect(() => {
    if (!chatboxVisible || !chatboxMounted || chatboxOpen) return;
    const element = chatboxRef.current;
    if (!element) return;
    const target = measureTargetChatboxHeight(element, expandedIntentRef.current);
    setChatboxHeight(target);
    if (prefersReducedMotion()) {
      setChatboxOpen(true);
      return;
    }
    const token = ++openTokenRef.current;
    animatingRef.current = true;
    void resizeCompactWindow(target).then(() => {
      if (openTokenRef.current !== token) return;
      requestAnimationFrame(() => {
        if (openTokenRef.current !== token) return;
        setChatboxOpen(true);
        clearPendingTimer();
        timerRef.current = window.setTimeout(() => {
          if (openTokenRef.current !== token) return;
          animatingRef.current = false;
          syncSteadyHeight();
        }, REVEAL_TRANSITION_FALLBACK_MS);
      });
    }, () => {
      if (openTokenRef.current !== token) return;
      animatingRef.current = false;
    });
  }, [chatboxVisible, chatboxMounted, chatboxOpen, clearPendingTimer, resizeCompactWindow, syncSteadyHeight]);

  useLayoutEffect(() => {
    const element = chatboxRef.current;
    if (!chatboxMounted || !element) return;
    const observer = new ResizeObserver(() => {
      if (animatingRef.current || !chatboxVisible || !chatboxOpen) return;
      syncSteadyHeight();
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [chatboxMounted, chatboxVisible, chatboxOpen, syncSteadyHeight]);

  useEffect(() => {
    void resizeCompactWindow(panelState).catch((reason) => {
      reportError(reason, "errors.window.compactResize", "window");
    });
  }, [panelState, resizeCompactWindow, reportError]);

  useEffect(() => () => {
    clearPendingTimer();
    clearUnlockWait();
  }, [clearPendingTimer, clearUnlockWait]);

  const handleExpandedChange = useCallback((next: boolean) => {
    if (next === expandedIntentRef.current) return;
    expandedIntentRef.current = next;
    clearPendingTimer();
    const element = chatboxRef.current;
    const token = ++expandTokenRef.current;
    if (!element || prefersReducedMotion()) {
      animatingRef.current = false;
      setExpanded(next);
      if (element) setChatboxHeight(measureTargetChatboxHeight(element, next));
      return;
    }
    if (next) {
      animatingRef.current = true;
      const target = measureTargetChatboxHeight(element, true);
      setChatboxHeight(target);
      void resizeCompactWindow(target).then(() => {
        if (expandTokenRef.current !== token || !expandedIntentRef.current) return;
        setExpanded(true);
        timerRef.current = window.setTimeout(() => {
          if (expandTokenRef.current !== token) return;
          animatingRef.current = false;
          syncSteadyHeight();
        }, REVEAL_TRANSITION_FALLBACK_MS);
      }, () => {
        if (expandTokenRef.current !== token) return;
        animatingRef.current = false;
        expandedIntentRef.current = expanded;
        syncSteadyHeight();
      });
      return;
    }
    if (!expanded) {
      animatingRef.current = false;
      setExpanded(false);
      setChatboxHeight(measureTargetChatboxHeight(element, false));
      return;
    }
    animatingRef.current = true;
    setExpanded(false);
    timerRef.current = window.setTimeout(() => {
      if (expandTokenRef.current !== token || expandedIntentRef.current) return;
      animatingRef.current = false;
      syncSteadyHeight();
    }, REVEAL_TRANSITION_FALLBACK_MS);
  }, [clearPendingTimer, expanded, resizeCompactWindow, syncSteadyHeight]);

  const handleRevealTransitionEnd = useCallback((event: TransitionEvent<HTMLDivElement>) => {
    if (event.propertyName !== "grid-template-rows") return;
    const target = event.target as HTMLElement;
    if (target.classList.contains("compact-chatbox-reveal")) {
      if (!chatboxVisible) {
        finishClose();
      } else {
        clearPendingTimer();
        animatingRef.current = false;
        syncSteadyHeight();
      }
      return;
    }
    if (target.classList.contains("chatbox-details-reveal")) {
      clearPendingTimer();
      animatingRef.current = false;
      syncSteadyHeight();
    }
  }, [chatboxVisible, clearPendingTimer, finishClose, syncSteadyHeight]);

  const closeChatbox = () => {
    chatbox.close();
    requestAnimationFrame(() => chatboxButtonRef.current?.focus());
  };
  const preset = COMPACT_FONT_PRESETS[compact.fontSize];
  const playerLimit = settings.value?.audio.output.mode === "vrchat_players"
    ? settings.value.audio.output.player_audio?.max_visible_players ?? 4
    : undefined;
  const style = {
    ...layoutVariables,
    "--compact-caption-height": `${Math.min(compact.height, COMPACT_WINDOW_MAX_HEIGHT - chatboxHeight)}px`,
    "--compact-background-opacity": compact.transparentBackground ? 0 : compact.backgroundOpacity / 100,
    "--compact-original-size": `${preset.original}px`,
    "--compact-original-line": `${preset.originalLine}px`,
    "--compact-translation-size": `${preset.translation}px`,
    "--compact-translation-line": `${preset.translationLine}px`,
  } as CSSProperties;

  return (
    <div style={style} className={`compact-root ${compact.transparentBackground ? "compact-root-transparent" : ""} ${overlayOpen ? "compact-root-overlay" : ""} ${selectionPanelOpen ? "compact-root-selection" : ""} ${(chatboxMounted || heightLocked) && !overlayOpen ? "compact-root-chatbox" : ""}`}>
      <CompactView
        hiddenTracks={playerDisplay.hiddenTracks}
        hiddenSourceLanguages={playerDisplay.hiddenSourceLanguages}
        transparentBackground={compact.transparentBackground}
        onToggleBackground={compact.toggleCompactBackground}
        backgroundOpacity={compact.backgroundOpacity}
        onBackgroundOpacityChange={compact.setBackgroundOpacity}
        fontSize={compact.fontSize}
        onFontSizeChange={compact.setFontSize}
        alwaysOnTop={compact.alwaysOnTop}
        alwaysOnTopDisabled={compact.alwaysOnTopAvailable === false}
        onToggleAlwaysOnTop={() => void compact.toggleAlwaysOnTop()}
        playerLimit={playerLimit}
        displayMode={playerLimit !== undefined ? compact.displayMode : undefined}
        onToggleDisplayMode={playerLimit !== undefined ? compact.toggleDisplayMode : undefined}
        onCaptionHeight={compact.ensureCaptionHeight}
        subtitles={compact.subtitles}
        subtitleHistory={compact.subtitleHistory}
        playerName={settings.value?.vrchat_player_id?.trim() || undefined}
        selectionTarget={selection.target}
        running={runtime.health?.capture_requested ?? false}
        vrchatMuted={runtime.vrchatMuteStatus?.muted === true}
        captureDisabled={!runtime.ready || capture.pending || (!runtime.health?.capture_requested && !capture.available)}
        captureReason={capture.unavailableReason}
        networkCaptureVisible={capture.networkCaptureVisible}
        networkCaptureRunning={runtime.health?.network_capture_requested ?? false}
        networkCaptureDisabled={!runtime.ready || capture.networkPending || (!runtime.health?.network_capture_requested && !capture.networkAvailable)}
        onNetworkCapture={() => void capture.toggleNetworkCapture()}
        chatboxOpen={chatboxVisible}
        chatboxDisabled={!runtime.ready || overlayOpen}
        chatboxButtonRef={chatboxButtonRef}
        onChatbox={settings.value ? () => {
          if (chatbox.open) closeChatbox();
          else {
            selection.clear();
            chatbox.show();
          }
        } : undefined}
        onSelect={selection.selectText}
        onCapture={() => void capture.toggleCapture()}
        onRestore={() => void compact.exitCompact(selection.clear)}
        onClose={() => void compact.closeWindow()}
        onResize={() => void compact.startCompactResize()}
      />
      {chatboxMounted && (
        <div
          className={`compact-chatbox-reveal${chatboxOpen ? " open" : ""}`}
          aria-hidden={!chatboxVisible}
          inert={!chatboxVisible}
          onTransitionEnd={handleRevealTransitionEnd}
        >
          <div className="compact-chatbox-clip">
            <div ref={chatboxRef} id="compact-chatbox" className="compact-chatbox">
              <ChatboxComposer
                inline
                expanded={expanded}
                onExpandedChange={handleExpandedChange}
                onClose={closeChatbox}
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
                onDraftChange={chatbox.setDraft}
                onTranslate={chatbox.translate}
                onSend={chatbox.send}
              />
            </div>
          </div>
        </div>
      )}
      <SelectionToolOverlays
        selection={selection}
        learning={learning}
        ankiEnabled={Boolean(settings.value?.features.anki && settings.value.anki.enabled)}
        compact
      />
    </div>
  );
}
