import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { ChatboxComposer } from "../chatbox/ChatboxComposer";
import { CompactView } from "../shell/CompactView";
import { COMPACT_FONT_PRESETS, COMPACT_LAYOUT } from "../shell/compact-layout";
import { COMPACT_WINDOW_MAX_HEIGHT } from "../shell/compact-mode";
import { SelectionToolOverlays } from "./SelectionToolOverlays";
import type { AppWorkspace } from "./useAppWorkspace";

const layoutVariables = Object.fromEntries(Object.entries(COMPACT_LAYOUT).map(([name, value]) => [
  `--compact-${name.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`)}`, `${value}px`,
]));

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
  const [expanded, setExpanded] = useState(false);
  const [chatboxHeight, setChatboxHeight] = useState(0);
  const chatboxVisible = chatbox.open && !overlayOpen;
  const panelState = overlayOpen ? true : chatboxVisible ? chatboxHeight : false;
  useLayoutEffect(() => {
    const element = chatboxRef.current;
    if (!chatboxVisible || !element) return;
    const measure = () => setChatboxHeight(Math.ceil(element.getBoundingClientRect().height));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [chatboxVisible]);
  useEffect(() => {
    void resizeCompactWindow(panelState).catch((reason) => {
      reportError(reason, "errors.window.compactResize", "window");
    });
  }, [panelState, resizeCompactWindow, reportError]);
  const closeChatbox = () => {
    chatbox.close();
    setExpanded(false);
    requestAnimationFrame(() => chatboxButtonRef.current?.focus());
  };
  const preset = COMPACT_FONT_PRESETS[compact.fontSize];
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
    <div style={style} className={`compact-root ${compact.transparentBackground ? "compact-root-transparent" : ""} ${overlayOpen ? "compact-root-overlay" : ""} ${selectionPanelOpen ? "compact-root-selection" : ""} ${chatboxVisible ? "compact-root-chatbox" : ""}`}>
      <CompactView
        hiddenTracks={playerDisplay.hiddenTracks}
        hiddenSourceLanguages={playerDisplay.hiddenSourceLanguages}
        transparentBackground={compact.transparentBackground}
        onToggleBackground={compact.toggleCompactBackground}
        backgroundOpacity={compact.backgroundOpacity}
        onBackgroundOpacityChange={compact.setBackgroundOpacity}
        fontSize={compact.fontSize}
        onFontSizeChange={compact.setFontSize}
        playerLimit={settings.value?.audio.output.mode === "vrchat_players" ? settings.value.audio.output.player_audio?.max_visible_players ?? 4 : undefined}
        onCaptionHeight={compact.ensureCaptionHeight}
        subtitles={compact.subtitles}
        subtitleHistory={compact.subtitleHistory}
        selectionTarget={selection.target}
        running={runtime.health?.capture_requested ?? false}
        vrchatMuted={runtime.vrchatMuteStatus?.muted === true}
        captureDisabled={!runtime.ready || capture.pending}
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
      {chatboxVisible && <div ref={chatboxRef} id="compact-chatbox" className="compact-chatbox">
        <ChatboxComposer
          inline
          expanded={expanded}
          onExpandedChange={setExpanded}
          onClose={closeChatbox}
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
      </div>}
      <SelectionToolOverlays
        selection={selection}
        learning={learning}
        ankiEnabled={Boolean(settings.value?.features.anki && settings.value.anki.enabled)}
        compact
      />
    </div>
  );
}
