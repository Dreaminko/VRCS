import { FolderOpen } from "lucide-react";
import { useTranslation } from "react-i18next";

import { NATIVE_APP } from "../../app/app-environment";
import type { SaveState } from "../settings-types";

export function ModelDirectorySetting({
  directoryText,
  saveState,
  disabled,
  busy,
  title,
  description,
  onSetDirectoryText,
  onUpdateDirectory,
  onChooseDirectory,
}: {
  directoryText: string;
  saveState: SaveState;
  disabled: boolean;
  busy: boolean;
  title: string;
  description: string;
  onSetDirectoryText: (value: string) => void;
  onUpdateDirectory: (value: string) => void;
  onChooseDirectory: () => Promise<void>;
}) {
  const { t } = useTranslation();
  return <div className="model-directory-setting">
    <label htmlFor="model-directory">
      <span>{title}</span>
      <small>{description}</small>
    </label>
    <div>
      <input
        id="model-directory"
        type="text"
        value={directoryText}
        disabled={disabled || busy || saveState === "saving"}
        spellCheck={false}
        onChange={(event) => onSetDirectoryText(event.target.value)}
        onBlur={() => onUpdateDirectory(directoryText)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            event.currentTarget.blur();
          }
        }}
      />
      <button
        className="secondary-button"
        type="button"
        disabled={!NATIVE_APP || disabled || busy || saveState === "saving"}
        data-tooltip={NATIVE_APP
          ? t("settings.recognition.chooseFolder")
          : t("settings.recognition.browserPathHint")}
        onClick={() => void onChooseDirectory()}
      >
        <FolderOpen size={16} />
        {t("settings.recognition.chooseFolder")}
      </button>
    </div>
  </div>;
}
