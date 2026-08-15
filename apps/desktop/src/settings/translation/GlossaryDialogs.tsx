import { FileUp, Plus, Trash2 } from "lucide-react";
import { useRef, type ChangeEvent, type RefObject } from "react";
import { useTranslation } from "react-i18next";

import type { GlossaryEntry } from "../../types";
import { SettingsDialog } from "../components/SettingsDialog";
import { Select } from "../SettingsControls";
import {
  characterCount,
  emptyGlossaryEntry,
  GLOSSARY_CATEGORIES,
  MAX_GLOSSARY_ENTRIES,
  MAX_GLOSSARY_NAME_LENGTH,
  MAX_GLOSSARY_TERM_LENGTH,
  MAX_GLOSSARY_URL_LENGTH,
  parsePublicGlossaryFile,
  validateEntries,
  type LocalGlossaryDraft,
  type SubscriptionGlossaryDraft,
} from "./glossary-utils";

export function LocalGlossaryDialog({
  draft,
  saving,
  error,
  returnFocusRef,
  onChange,
  onError,
  onSave,
  onClose,
}: {
  draft: LocalGlossaryDraft;
  saving: boolean;
  error: string;
  returnFocusRef: RefObject<HTMLButtonElement | null>;
  onChange: (draft: LocalGlossaryDraft) => void;
  onError: (error: string) => void;
  onSave: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const fileInputRef = useRef<HTMLInputElement>(null);

  const importJson = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    try {
      let value: unknown;
      try {
        value = JSON.parse(await file.text()) as unknown;
      } catch {
        onError(t("settings.translation.glossaryValidation.importInvalid"));
        return;
      }
      const parsed = parsePublicGlossaryFile(value);
      if (!parsed) {
        onError(t("settings.translation.glossaryValidation.importInvalid"));
        return;
      }
      const importedName = parsed.name?.trim();
      if (importedName && characterCount(importedName) > MAX_GLOSSARY_NAME_LENGTH) {
        onError(t("settings.translation.glossaryValidation.nameTooLong"));
        return;
      }
      const validationError = validateEntries(parsed.entries, t);
      if (validationError) {
        onError(validationError);
        return;
      }
      onChange({
        ...draft,
        name: importedName || draft.name,
        entries: parsed.entries,
      });
    } catch {
      onError(t("settings.translation.glossaryValidation.importInvalid"));
    }
  };

  const updateEntry = (index: number, patch: Partial<GlossaryEntry>) => onChange({
    ...draft,
    entries: draft.entries.map((entry, entryIndex) => entryIndex === index
      ? { ...entry, ...patch }
      : entry),
  });

  return (
    <SettingsDialog
      label={t(draft.id ? "settings.translation.editGlossary" : "settings.translation.addGlossary")}
      saving={saving}
      returnFocusRef={returnFocusRef}
      className="glossary-dialog"
      autoFocus
      onClose={onClose}
    >
      <form
        className="api-profile-editor glossary-dialog-form"
        onSubmit={(event) => {
          event.preventDefault();
          onSave();
        }}
      >
        <div className="api-profile-editor-heading">
          <strong>{t(draft.id ? "settings.translation.editGlossary" : "settings.translation.addGlossary")}</strong>
          <small>{t("settings.translation.localGlossaryDialogHint")}</small>
        </div>
        <div className="api-profile-editor-content">
          <label className="field cloud-text-field">
            <span>{t("settings.translation.glossaryName")}</span>
            <input
              maxLength={MAX_GLOSSARY_NAME_LENGTH}
              value={draft.name}
              disabled={saving}
              placeholder={t("settings.translation.glossaryNamePlaceholder")}
              onChange={(event) => onChange({ ...draft, name: event.target.value })}
            />
          </label>
          <div className="glossary-dialog-toolbar">
            <input
              className="glossary-file-input"
              ref={fileInputRef}
              type="file"
              accept="application/json,.json"
              disabled={saving}
              onChange={(event) => void importJson(event)}
            />
            <button
              className="secondary-button"
              type="button"
              disabled={saving}
              onClick={() => fileInputRef.current?.click()}
            >
              <FileUp size={14} aria-hidden="true" />
              {t("settings.translation.importGlossaryJson")}
            </button>
            <button
              className="secondary-button"
              type="button"
              disabled={saving || draft.entries.length >= MAX_GLOSSARY_ENTRIES}
              onClick={() => onChange({ ...draft, entries: [...draft.entries, emptyGlossaryEntry()] })}
            >
              <Plus size={14} aria-hidden="true" />
              {t("settings.translation.addGlossaryEntry")}
            </button>
            <small>{t("settings.translation.glossaryEntryLimit", { count: MAX_GLOSSARY_ENTRIES })}</small>
          </div>
          <div className="glossary-dialog-entry-list">
            {draft.entries.length === 0 && (
              <p className="glossary-dialog-empty">{t("settings.translation.noGlossaryEntries")}</p>
            )}
            {draft.entries.map((entry, index) => (
              <div className="glossary-dialog-entry" key={index}>
                <label className="field">
                  <span>{t("settings.translation.glossarySource")}</span>
                  <input
                    maxLength={MAX_GLOSSARY_TERM_LENGTH}
                    value={entry.source}
                    disabled={saving}
                    onChange={(event) => updateEntry(index, { source: event.target.value })}
                  />
                </label>
                <label className="field">
                  <span>{t("settings.translation.glossaryTarget")}</span>
                  <input
                    maxLength={MAX_GLOSSARY_TERM_LENGTH}
                    value={entry.target ?? ""}
                    disabled={saving || entry.target === null}
                    placeholder={entry.target === null ? t("settings.translation.keepOriginal") : undefined}
                    onChange={(event) => updateEntry(index, { target: event.target.value })}
                  />
                </label>
                <Select
                  label={t("settings.translation.glossaryCategory")}
                  value={entry.category}
                  disabled={saving}
                  floating="dialog"
                  options={GLOSSARY_CATEGORIES.map((category) => ({
                    value: category,
                    label: t(`settings.translation.glossaryCategories.${category}`),
                  }))}
                  onChange={(category) => updateEntry(index, {
                    category: category as GlossaryEntry["category"],
                  })}
                />
                <div className="glossary-dialog-entry-options">
                  <label className="translation-glossary-check">
                    <input
                      type="checkbox"
                      checked={entry.target === null}
                      disabled={saving}
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
                      disabled={saving}
                      onChange={(event) => updateEntry(index, {
                        case_sensitive: event.target.checked,
                      })}
                    />
                    {t("settings.translation.caseSensitive")}
                  </label>
                  <button
                    className="api-profile-header-delete"
                    type="button"
                    aria-label={t("common.delete")}
                    disabled={saving}
                    onClick={() => onChange({
                      ...draft,
                      entries: draft.entries.filter((_, entryIndex) => entryIndex !== index),
                    })}
                  >
                    <Trash2 size={15} aria-hidden="true" />
                  </button>
                </div>
              </div>
            ))}
          </div>
          <small className="glossary-dialog-error api-model-catalog-error" aria-live="polite">{error}</small>
        </div>
        <div className="api-profile-editor-actions">
          <div />
          <div className="settings-inline-actions">
            <button className="secondary-button" type="button" disabled={saving} onClick={onClose}>
              {t("common.cancel")}
            </button>
            <button className="primary-button" type="submit" disabled={saving}>
              {saving ? t("common.loading") : t("common.save")}
            </button>
          </div>
        </div>
      </form>
    </SettingsDialog>
  );
}

export function SubscriptionGlossaryDialog({
  draft,
  saving,
  error,
  returnFocusRef,
  onChange,
  onSave,
  onClose,
}: {
  draft: SubscriptionGlossaryDraft;
  saving: boolean;
  error: string;
  returnFocusRef: RefObject<HTMLButtonElement | null>;
  onChange: (draft: SubscriptionGlossaryDraft) => void;
  onSave: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  return (
    <SettingsDialog
      label={t(draft.id ? "settings.translation.editSubscription" : "settings.translation.addSubscription")}
      saving={saving}
      returnFocusRef={returnFocusRef}
      className="glossary-dialog"
      autoFocus
      onClose={onClose}
    >
      <form
        className="api-profile-editor glossary-dialog-form glossary-subscription-dialog-form"
        onSubmit={(event) => {
          event.preventDefault();
          onSave();
        }}
      >
        <div className="api-profile-editor-heading">
          <strong>{t(draft.id ? "settings.translation.editSubscription" : "settings.translation.addSubscription")}</strong>
          <small>{t("settings.translation.subscriptionDialogHint")}</small>
        </div>
        <div className="api-profile-editor-content">
          <label className="field cloud-text-field">
            <span>{t("settings.translation.glossaryUrl")}</span>
            <input
              type="url"
              maxLength={MAX_GLOSSARY_URL_LENGTH}
              value={draft.url}
              disabled={saving}
              spellCheck={false}
              placeholder="https://example.com/glossary.json"
              onChange={(event) => onChange({ ...draft, url: event.target.value })}
            />
            <small>{t("settings.translation.glossaryUrlHint")}</small>
          </label>
          <label className="field cloud-text-field">
            <span>{t("settings.translation.glossaryDisplayName")}</span>
            <input
              maxLength={MAX_GLOSSARY_NAME_LENGTH}
              value={draft.displayName}
              disabled={saving}
              placeholder={t("settings.translation.glossaryDisplayNamePlaceholder")}
              onChange={(event) => onChange({ ...draft, displayName: event.target.value })}
            />
          </label>
          <small className="glossary-dialog-error api-model-catalog-error" aria-live="polite">{error}</small>
        </div>
        <div className="api-profile-editor-actions">
          <div />
          <div className="settings-inline-actions">
            <button className="secondary-button" type="button" disabled={saving} onClick={onClose}>
              {t("common.cancel")}
            </button>
            <button className="primary-button" type="submit" disabled={saving}>
              {saving ? t("common.loading") : t("common.save")}
            </button>
          </div>
        </div>
      </form>
    </SettingsDialog>
  );
}
