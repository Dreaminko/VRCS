import type { RefObject } from "react";
import { GraduationCap } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { DictionarySource } from "../../types";
import type { SaveState } from "../settings-types";
import { DictionarySettingsSection } from "./DictionarySettingsSection";
import { LearningAiSettingsSection } from "./LearningAiSettingsSection";

export function LearningSettingsSection({
  locale,
  selectionLookupEnabled,
  dictionaries,
  dictionaryBusy,
  dictionaryMessage,
  dictionaryProgress,
  dictionaryFileRef,
  saveState,
  onSelectionLookupChange,
  onChooseDictionary,
  onRemoveDictionary,
}: {
  locale: string;
  selectionLookupEnabled: boolean;
  dictionaries: DictionarySource[];
  dictionaryBusy: boolean;
  dictionaryMessage: string;
  dictionaryProgress: number | null;
  dictionaryFileRef: RefObject<HTMLInputElement | null>;
  saveState: SaveState;
  onSelectionLookupChange: (enabled: boolean) => void;
  onChooseDictionary: (file?: File) => Promise<void>;
  onRemoveDictionary: (dictionary: DictionarySource) => Promise<void>;
}) {
  const { t } = useTranslation();

  return (
    <div className="settings-section settings-section-active learning-section" id="settings-panel-learning" role="tabpanel" aria-labelledby="settings-tab-learning">
      <div className="section-heading learning-page-heading">
        <div>
          <GraduationCap size={18} />
          <h2>{t("settings.learning.title")}</h2>
        </div>
      </div>
      <div className="learning-settings-list">
        <LearningAiSettingsSection />
        <DictionarySettingsSection
          locale={locale}
          selectionLookupEnabled={selectionLookupEnabled}
          dictionaries={dictionaries}
          busy={dictionaryBusy}
          message={dictionaryMessage}
          progress={dictionaryProgress}
          fileInputRef={dictionaryFileRef}
          saveState={saveState}
          onSelectionLookupChange={onSelectionLookupChange}
          onChoose={onChooseDictionary}
          onRemove={onRemoveDictionary}
        />
      </div>
    </div>
  );
}
