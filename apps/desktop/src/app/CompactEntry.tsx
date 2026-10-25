import type { CSSProperties } from "react";
import { CompactView } from "../shell/CompactView";
import { COMPACT_FONT_PRESETS, COMPACT_LAYOUT } from "../shell/compact-layout";
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
}: {
  compact: AppWorkspace["compact"];
  runtime: AppWorkspace["runtime"];
  capture: AppWorkspace["capture"];
  settings: AppWorkspace["settings"];
  selection: AppWorkspace["selection"];
  learning: AppWorkspace["learning"];
  playerDisplay: AppWorkspace["playerDisplay"];
}) {
  const { selectionPanelOpen, overlayOpen } = compact;
  const preset = COMPACT_FONT_PRESETS[compact.fontSize];
  const style = {
    ...layoutVariables,
    "--compact-background-opacity": compact.transparentBackground ? 0 : compact.backgroundOpacity / 100,
    "--compact-original-size": `${preset.original}px`,
    "--compact-original-line": `${preset.originalLine}px`,
    "--compact-translation-size": `${preset.translation}px`,
    "--compact-translation-line": `${preset.translationLine}px`,
  } as CSSProperties;

  return (
    <div style={style} className={`compact-root ${compact.transparentBackground ? "compact-root-transparent" : ""} ${overlayOpen ? "compact-root-overlay" : ""} ${selectionPanelOpen ? "compact-root-selection" : ""}`}>
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
        onSelect={selection.selectText}
        onCapture={() => void capture.toggleCapture()}
        onRestore={() => void compact.exitCompact(selection.clear)}
        onClose={() => void compact.closeWindow()}
        onResize={() => void compact.startCompactResize()}
      />
      <SelectionToolOverlays
        selection={selection}
        learning={learning}
        ankiEnabled={Boolean(settings.value?.features.anki && settings.value.anki.enabled)}
        compact
      />
    </div>
  );
}
