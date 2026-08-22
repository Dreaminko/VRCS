import { CompactView } from "../shell/CompactView";
import { SelectionToolOverlays } from "./SelectionToolOverlays";
import type { AppWorkspace } from "./useAppWorkspace";

export function CompactEntry({
  compact,
  runtime,
  capture,
  settings,
  selection,
  learning,
}: {
  compact: AppWorkspace["compact"];
  runtime: AppWorkspace["runtime"];
  capture: AppWorkspace["capture"];
  settings: AppWorkspace["settings"];
  selection: AppWorkspace["selection"];
  learning: AppWorkspace["learning"];
}) {
  const selectionPanelOpen = Boolean(selection.target && selection.tool);

  return (
    <div className={`compact-root ${selectionPanelOpen ? "compact-root-selection" : ""}`}>
      <CompactView
        subtitle={compact.subtitle}
        running={runtime.health?.capture_requested ?? false}
        vrchatMuted={runtime.vrchatMuteStatus?.muted === true}
        captureDisabled={!runtime.ready || capture.pending}
        onSelect={selection.selectText}
        onCapture={() => void capture.toggleCapture()}
        onRestore={() => void compact.exitCompact(selection.clear)}
        onClose={() => void compact.closeWindow()}
      />
      <SelectionToolOverlays
        selection={selection}
        learning={learning}
        ankiEnabled={settings.value?.anki.enabled ?? true}
        compact
      />
    </div>
  );
}
