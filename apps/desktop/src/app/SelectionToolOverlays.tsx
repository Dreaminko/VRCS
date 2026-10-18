import { DictionaryPopover } from "../dictionary/DictionaryPopover";
import { SelectionAiPopover } from "../selection/SelectionAiPopover";
import type { AppWorkspace } from "./useAppWorkspace";

export function SelectionToolOverlays({
  selection,
  learning,
  ankiEnabled,
  compact = false,
}: {
  selection: AppWorkspace["selection"];
  learning: AppWorkspace["learning"];
  ankiEnabled: boolean;
  compact?: boolean;
}) {
  return (
    <>
      {selection.lookup && selection.tool === "dictionary" && (
        <DictionaryPopover
          lookup={selection.lookup}
          loading={selection.lookupLoading}
          ankiEnabled={ankiEnabled}
          compact={compact || undefined}
          onAskAi={() => void selection.openAi()}
          onAddLearning={learning.workspace.collectLookup}
          onClose={selection.close}
        />
      )}
      {selection.target && selection.tool === "ai" && (
        <SelectionAiPopover
          target={selection.target}
          preferences={learning.workspace.preferences}
          compact={compact || undefined}
          onBack={selection.lookup ? selection.returnToDictionary : undefined}
          onAddLearning={() => learning.workspace.collectLookup(
            selection.lookup ?? {
              ...selection.target!,
              term: selection.target!.selectedText,
              entries: [],
            },
          )}
          onConfigure={() => void selection.openAiSettings()}
          onClose={selection.close}
        />
      )}
    </>
  );
}
