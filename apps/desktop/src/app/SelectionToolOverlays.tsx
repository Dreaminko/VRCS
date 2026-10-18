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
          onAddLearning={async () => {
            // 在点击时捕获当前 target，并且只复用属于同一选区的查词结果。
            const target = selection.target;
            if (!target) return null;
            const lookup = selection.lookup
              && selection.lookup.selectedText === target.selectedText
              && selection.lookup.context === target.context
              ? selection.lookup
              : null;
            return learning.workspace.collectLookup(lookup ?? { ...target, term: target.selectedText, entries: [] });
          }}
          onConfigure={() => void selection.openAiSettings()}
          onClose={selection.close}
        />
      )}
    </>
  );
}
