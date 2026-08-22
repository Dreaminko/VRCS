import type { RefObject } from "react";
import { useTranslation } from "react-i18next";
import { BookOpen, Trash2, Upload } from "lucide-react";

import type { DictionarySource } from "../../dictionary/types";
import type { SaveState } from "../settings-types";
import { PreferenceToggle } from "../SettingsControls";

export function DictionarySettingsSection({
  locale,
  selectionLookupEnabled,
  dictionaries,
  busy,
  message,
  progress,
  fileInputRef,
  saveState,
  onSelectionLookupChange,
  onChoose,
  onRemove,
}: {
  locale: string;
  selectionLookupEnabled: boolean;
  dictionaries: DictionarySource[];
  busy: boolean;
  message: string;
  progress: number | null;
  fileInputRef: RefObject<HTMLInputElement | null>;
  saveState: SaveState;
  onSelectionLookupChange: (enabled: boolean) => void;
  onChoose: (file?: File) => Promise<void>;
  onRemove: (dictionary: DictionarySource) => Promise<void>;
}) {
  const { t } = useTranslation();
  const dictionaryBusy = busy;
  const dictionaryMessage = message;
  const dictionaryProgress = progress === null ? null : Math.max(0, Math.min(1, progress));
  const progressPercentage = dictionaryProgress === null ? null : Math.round(dictionaryProgress * 100);
  const dictionaryFileRef = fileInputRef;
  const chooseDictionary = onChoose;
  const removeDictionary = onRemove;
  return (
        <section className="learning-settings-group dictionary-section" aria-labelledby="learning-dictionary-title">
          <div className="section-heading">
            <div><BookOpen size={18} /><h3 id="learning-dictionary-title">{t("settings.dictionary.title")}</h3><span>{dictionaries.length ? t("settings.dictionary.importedCount", { count: dictionaries.length }) : t("settings.dictionary.noneImported")}</span></div>
            <button className="secondary-button" type="button" disabled={dictionaryBusy} onClick={() => dictionaryFileRef.current?.click()}><Upload size={15} />{t("settings.dictionary.importYomitan")}</button>
            <input
              ref={dictionaryFileRef}
              className="dictionary-file-input"
              type="file"
              accept=".zip,application/zip"
              onChange={(event) => void chooseDictionary(event.target.files?.[0])}
            />
          </div>
          <div className="settings-toggle-list settings-feature-toggle">
            <PreferenceToggle
              title={t("settings.dictionary.selectionLookup")}
              checked={selectionLookupEnabled}
              disabled={saveState === "saving"}
              onChange={onSelectionLookupChange}
            />
          </div>
          {dictionaries.length ? (
            <div className="dictionary-source-list">
              {dictionaries.map((dictionary) => (
                <div className="dictionary-source-row" key={dictionary.id}>
                  <div className="dictionary-source-icon"><BookOpen size={17} /></div>
                  <div>
                    <strong>{dictionary.title}</strong>
                    <span>{dictionary.source_language.toUpperCase()}{dictionary.target_language ? ` → ${dictionary.target_language.toUpperCase()}` : ""} · {t("settings.dictionary.entryCount", { count: dictionary.entry_count, formatted: new Intl.NumberFormat(locale).format(dictionary.entry_count) })} · {dictionary.revision}</span>
                  </div>
                  <button type="button" disabled={dictionaryBusy} aria-label={t("settings.dictionary.removeNamed", { title: dictionary.title })} title={t("settings.dictionary.remove")} onClick={() => void removeDictionary(dictionary)}><Trash2 size={16} /></button>
                </div>
              ))}
            </div>
          ) : <p className="dictionary-empty">{t("settings.dictionary.emptyHint")}</p>}
          {dictionaryMessage && <p className="dictionary-feedback" role="status">{dictionaryMessage}</p>}
          {dictionaryBusy && dictionaryProgress !== null && progressPercentage !== null && (
            <div className="dictionary-import-progress">
              <div
                className="dictionary-progress-track"
                role="progressbar"
                aria-label={t("settings.dictionary.progressLabel")}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={progressPercentage}
              >
                <span style={{ transform: `scaleX(${dictionaryProgress})` }} />
              </div>
              <span aria-hidden="true">{progressPercentage}%</span>
            </div>
          )}
        </section>
  );
}
