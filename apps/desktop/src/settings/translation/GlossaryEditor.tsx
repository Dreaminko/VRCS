import { Plus, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { GlossaryEntry } from "../../types";
import { Select } from "../SettingsControls";

export function GlossaryEditor({ glossary, disabled, onChange }: {
  glossary: GlossaryEntry[];
  disabled: boolean;
  onChange: (glossary: GlossaryEntry[]) => void;
}) {
  const { t } = useTranslation();
  const updateEntry = (index: number, patch: Partial<GlossaryEntry>) => onChange(
    glossary.map((entry, entryIndex) => (
      entryIndex === index ? { ...entry, ...patch } : entry
    )),
  );

  return (
    <>
      <div className="translation-glossary-heading">
        <span>
          <strong>{t("settings.translation.glossary")}</strong>
          <small>{t("settings.translation.glossaryDescription")}</small>
        </span>
        <button
          className="secondary-button"
          type="button"
          disabled={disabled || glossary.length >= 500}
          onClick={() => onChange([...glossary, {
            source: "",
            target: null,
            category: "custom",
            case_sensitive: false,
          }])}
        >
          <Plus size={14} />
          {t("settings.translation.addGlossary")}
        </button>
      </div>
      {glossary.map((entry, index) => (
        <div className="translation-glossary-entry" key={index}>
          <input
            aria-label={t("settings.translation.glossarySource")}
            maxLength={200}
            placeholder={t("settings.translation.glossarySource")}
            value={entry.source}
            disabled={disabled}
            onChange={(event) => updateEntry(index, { source: event.target.value })}
          />
          <input
            aria-label={t("settings.translation.glossaryTarget")}
            maxLength={200}
            placeholder={entry.target === null
              ? t("settings.translation.keepOriginal")
              : t("settings.translation.glossaryTarget")}
            value={entry.target ?? ""}
            disabled={disabled || entry.target === null}
            onChange={(event) => updateEntry(index, { target: event.target.value })}
          />
          <Select
            label={t("settings.translation.glossaryCategory")}
            value={entry.category}
            disabled={disabled}
            options={["person", "world", "game", "custom"].map((category) => ({
              value: category,
              label: t(`settings.translation.glossaryCategories.${category}`),
            }))}
            onChange={(category) => updateEntry(index, {
              category: category as GlossaryEntry["category"],
            })}
          />
          <label className="translation-glossary-check">
            <input
              type="checkbox"
              checked={entry.target === null}
              disabled={disabled}
              onChange={(event) => updateEntry(index, {
                target: event.target.checked ? null : "",
              })}
            />
            {t("settings.translation.keepOriginal")}
          </label>
          <label className="translation-glossary-check">
            <input
              type="checkbox"
              checked={entry.case_sensitive}
              disabled={disabled}
              onChange={(event) => updateEntry(index, {
                case_sensitive: event.target.checked,
              })}
            />
            {t("settings.translation.caseSensitive")}
          </label>
          <button
            className="icon-button"
            type="button"
            aria-label={t("common.delete")}
            disabled={disabled}
            onClick={() => onChange(glossary.filter((_, entryIndex) => entryIndex !== index))}
          >
            <Trash2 size={15} />
          </button>
        </div>
      ))}
    </>
  );
}
