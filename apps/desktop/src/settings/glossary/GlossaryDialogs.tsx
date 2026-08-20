import { useRef, type ChangeEvent, type RefObject } from "react";
import { useTranslation } from "react-i18next";

import { SettingsDialog } from "../components/SettingsDialog";
import { GlossaryEntryTable } from "./GlossaryEntryTable";
import {
  characterCount,
  MAX_GLOSSARY_NAME_LENGTH,
  MAX_GLOSSARY_URL_LENGTH,
  glossaryEntryDraft,
  glossaryEntryValue,
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
  const initialSnapshotRef = useRef(JSON.stringify({
    name: draft.name,
    entries: draft.entries.map(glossaryEntryValue),
  }));
  const dirty = initialSnapshotRef.current !== JSON.stringify({
    name: draft.name,
    entries: draft.entries.map(glossaryEntryValue),
  });
  const entryValidationError = validateEntries(draft.entries, t);

  const requestClose = () => {
    if (dirty && !window.confirm(t("settings.glossary.table.confirmDiscardChanges"))) return;
    onClose();
  };

  const importJson = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    try {
      let value: unknown;
      try {
        value = JSON.parse(await file.text()) as unknown;
      } catch {
        onError(t("settings.glossary.glossaryValidation.importInvalid"));
        return;
      }
      const parsed = parsePublicGlossaryFile(value);
      if (!parsed) {
        onError(t("settings.glossary.glossaryValidation.importInvalid"));
        return;
      }
      const importedName = parsed.name?.trim();
      if (importedName && characterCount(importedName) > MAX_GLOSSARY_NAME_LENGTH) {
        onError(t("settings.glossary.glossaryValidation.nameTooLong"));
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
        entries: parsed.entries.map(glossaryEntryDraft),
      });
    } catch {
      onError(t("settings.glossary.glossaryValidation.importInvalid"));
    }
  };

  return (
    <SettingsDialog
      label={t(draft.id ? "settings.glossary.editGlossary" : "settings.glossary.addGlossary")}
      saving={saving}
      returnFocusRef={returnFocusRef}
      className="glossary-dialog glossary-local-dialog"
      autoFocus
      onClose={requestClose}
    >
      <form
        className="api-profile-editor glossary-dialog-form"
        onSubmit={(event) => {
          event.preventDefault();
          onSave();
        }}
      >
        <div className="api-profile-editor-heading">
          <strong>{t(draft.id ? "settings.glossary.editGlossary" : "settings.glossary.addGlossary")}</strong>
        </div>
        <div className="api-profile-editor-content" data-floating-boundary>
          <label className="field cloud-text-field">
            <span>{t("settings.glossary.glossaryName")}</span>
            <input
              maxLength={MAX_GLOSSARY_NAME_LENGTH}
              value={draft.name}
              disabled={saving}
              placeholder={t("settings.glossary.glossaryNamePlaceholder")}
              onChange={(event) => onChange({ ...draft, name: event.target.value })}
            />
          </label>
          <input
            className="glossary-file-input"
            ref={fileInputRef}
            type="file"
            accept="application/json,.json"
            disabled={saving}
            onChange={(event) => void importJson(event)}
          />
          <GlossaryEntryTable
            entries={draft.entries}
            fileInputRef={fileInputRef}
            saving={saving}
            showValidation={Boolean(error && error === entryValidationError)}
            onChange={(entries) => onChange({ ...draft, entries })}
          />
          <small className="glossary-dialog-error api-model-catalog-error" aria-live="polite">{error}</small>
        </div>
        <div className="api-profile-editor-actions">
          <div />
          <div className="settings-inline-actions">
            <button className="secondary-button" type="button" disabled={saving} onClick={requestClose}>
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
      label={t(draft.id ? "settings.glossary.editSubscription" : "settings.glossary.addSubscription")}
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
          <strong>{t(draft.id ? "settings.glossary.editSubscription" : "settings.glossary.addSubscription")}</strong>
        </div>
        <div className="api-profile-editor-content">
          <label className="field cloud-text-field">
            <span>{t("settings.glossary.glossaryUrl")}</span>
            <input
              type="url"
              maxLength={MAX_GLOSSARY_URL_LENGTH}
              value={draft.url}
              disabled={saving}
              spellCheck={false}
              placeholder="https://example.com/glossary.json"
              onChange={(event) => onChange({ ...draft, url: event.target.value })}
            />
          </label>
          <label className="field cloud-text-field">
            <span>{t("settings.glossary.glossaryDisplayName")}</span>
            <input
              maxLength={MAX_GLOSSARY_NAME_LENGTH}
              value={draft.displayName}
              disabled={saving}
              placeholder={t("settings.glossary.glossaryDisplayNamePlaceholder")}
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
