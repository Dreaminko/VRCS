import { Plus, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import {
  apiProfileDraftCanSave,
  createApiProfileDraft,
  toggleDraftCapability,
  type ApiProfileEditorDraft,
} from "../../api-profile-draft";
import {
  groupedProviderOptions,
  providerCapabilities,
  providerDefinition,
} from "../../provider-catalog";
import type {
  ApiAuthMode,
  ApiCapability,
  ApiProfileView,
  ProviderConnectionField,
  ProviderDefinition,
} from "../../providers/types";
import { Select } from "../SettingsControls";
import { ApiCapabilitySelector } from "./ApiCapabilitySelector";

export type { ApiProfileEditorDraft } from "../../api-profile-draft";

export function ApiProfileEditor({
  draft,
  saving,
  credential,
  providerDefinitions,
  requireCredential = false,
  requiredCapability,
  floatingSelects = false,
  onChange,
  onSave,
  onCancel,
  onRemoveCredential,
}: {
  draft: ApiProfileEditorDraft;
  saving: boolean;
  credential?: ApiProfileView["credential"];
  providerDefinitions: ProviderDefinition[];
  requireCredential?: boolean;
  requiredCapability?: ApiCapability;
  floatingSelects?: boolean;
  onChange: (draft: ApiProfileEditorDraft) => void;
  onSave: () => void;
  onCancel?: () => void;
  onRemoveCredential?: () => void;
}) {
  const { t } = useTranslation();
  const editing = Boolean(draft.id);
  const definition = providerDefinition(providerDefinitions, draft.provider);
  const availableCapabilities = providerCapabilities(definition);
  const eligibleDefinitions = requiredCapability
    ? providerDefinitions.filter((item) => providerCapabilities(item).includes(requiredCapability))
    : providerDefinitions;
  const credentialAvailable = credential?.configured || Boolean(draft.api_key.trim());
  const canSave = (!requiredCapability || draft.enabled_capabilities.includes(requiredCapability)) && apiProfileDraftCanSave(
    draft,
    definition,
    Boolean(credential?.configured),
    requireCredential,
  ) && !saving;
  const providerOptions = groupedProviderOptions(eligibleDefinitions, {
    cloud_provider: t("settings.apiManagement.providerCategories.cloudProvider"),
    local_service: t("settings.apiManagement.providerCategories.localService"),
    custom_protocol: t("settings.apiManagement.providerCategories.customProtocol"),
  });
  const showAdvancedHttp = definition?.category === "custom_protocol";

  return (
    <form className="api-profile-editor" onSubmit={(event) => { event.preventDefault(); onSave(); }}>
      <div className="api-profile-editor-heading">
        <strong>{t(editing ? "settings.apiManagement.editProfile" : "settings.apiManagement.addProfile")}</strong>
      </div>
      <div className="api-profile-editor-content" data-floating-boundary>
        <div className="api-profile-form-grid">
          <label className="field cloud-text-field">
            <span>{t("settings.apiManagement.profileName")}</span>
            <input
              value={draft.name}
              maxLength={50}
              autoFocus
              disabled={saving}
              placeholder={t("settings.apiManagement.profileNamePlaceholder")}
              onChange={(event) => onChange({ ...draft, name: event.target.value })}
            />
          </label>
          <Select
            label={t("settings.apiManagement.provider")}
            value={draft.provider}
            options={providerOptions}
            disabled={editing || saving || providerOptions.length === 0}
            floating={floatingSelects ? "dialog" : undefined}
            onChange={(provider) => {
              const next = createApiProfileDraft(eligibleDefinitions, provider);
              onChange({
                ...next,
                name: draft.name,
                enabled_capabilities: requiredCapability && !next.enabled_capabilities.includes(requiredCapability)
                  ? [...next.enabled_capabilities, requiredCapability]
                  : next.enabled_capabilities,
              });
            }}
          />

          {availableCapabilities.length > 1 && (
            <ApiCapabilitySelector
              available={availableCapabilities}
              enabled={draft.enabled_capabilities}
              disabled={saving}
              requiredCapability={requiredCapability}
              onToggle={(capability) => onChange(toggleDraftCapability(draft, capability))}
            />
          )}

          {definition?.connection.base_url.mode === "editable" && (
            <label className="field cloud-text-field api-compatible-url-field">
              <span>{t("settings.apiManagement.baseUrl")}</span>
              <input
                type="url"
                value={draft.base_url}
                disabled={saving}
                spellCheck={false}
                placeholder={definition.connection.base_url.default ?? "https://api.example.com/v1"}
                onChange={(event) => onChange({ ...draft, base_url: event.target.value })}
              />
            </label>
          )}

          {(definition?.connection.fields ?? []).map((field) => (
            <ConnectionField
              key={field.id}
              field={field}
              value={draft.fields[field.id] ?? ""}
              disabled={saving}
              floating={floatingSelects ? "dialog" : undefined}
              onChange={(value) => onChange({
                ...draft,
                fields: { ...draft.fields, [field.id]: value },
              })}
            />
          ))}

          {(definition?.connection.auth_modes.length ?? 0) > 1 && (
            <Select
              label={t("settings.apiManagement.authentication")}
              value={draft.auth_mode}
              options={(definition?.connection.auth_modes ?? []).map((mode) => ({
                value: mode,
                label: t(`settings.apiManagement.authModes.${mode}`),
              }))}
              disabled={saving}
              floating={floatingSelects ? "dialog" : undefined}
              onChange={(value) => onChange({ ...draft, auth_mode: value as ApiAuthMode })}
            />
          )}

          {showAdvancedHttp && (
            <label className="field cloud-text-field">
              <span>{t("settings.apiManagement.timeout")}</span>
              <input
                type="number"
                min={1000}
                max={120000}
                step={1000}
                value={draft.timeout_ms}
                disabled={saving}
                onChange={(event) => onChange({ ...draft, timeout_ms: Number(event.target.value) })}
              />
            </label>
          )}

          {draft.auth_mode !== "none" && (
            <label className="field cloud-text-field api-profile-key-field">
              <span>{t("settings.apiManagement.apiKey")}</span>
              <input
                type="password"
                value={draft.api_key}
                autoComplete="off"
                spellCheck={false}
                disabled={saving || credential?.environment_override}
                placeholder={credential?.configured
                  ? t("settings.apiManagement.credentialConfigured")
                  : requireCredential
                    ? t("onboarding.recognition.apiKeyRequired")
                    : t("settings.apiManagement.apiKeyOptional")}
                aria-invalid={requireCredential && !credentialAvailable}
                onChange={(event) => onChange({ ...draft, api_key: event.target.value })}
              />
            </label>
          )}
        </div>

        {showAdvancedHttp && (
          <div className="api-profile-headers-field">
            <div className="api-profile-headers-heading">
              <span>{t("settings.apiManagement.customHeaders")}</span>
              <button className="secondary-button api-add-header-button" type="button" disabled={saving || draft.headers.length >= 16} onClick={() => onChange({
                ...draft,
                headers: [...draft.headers, { name: "", value: "" }],
              })}>
                <Plus size={14} aria-hidden="true" />
                {t("settings.apiManagement.addHeader")}
              </button>
            </div>
            <small>{t("settings.apiManagement.customHeadersWarning")}</small>
            <div className="api-profile-header-list">
              {draft.headers.map((header, index) => (
                <div className="api-profile-header-row" key={index}>
                  <input
                    value={header.name}
                    disabled={saving}
                    placeholder="HTTP-Referer"
                    onChange={(event) => onChange({
                      ...draft,
                      headers: draft.headers.map((item, itemIndex) => itemIndex === index
                        ? { ...item, name: event.target.value }
                        : item),
                    })}
                  />
                  <input
                    value={header.value}
                    disabled={saving}
                    placeholder="https://example.com"
                    onChange={(event) => onChange({
                      ...draft,
                      headers: draft.headers.map((item, itemIndex) => itemIndex === index
                        ? { ...item, value: event.target.value }
                        : item),
                    })}
                  />
                  <button
                    className="api-profile-header-delete"
                    type="button"
                    aria-label={t("common.delete")}
                    disabled={saving}
                    onClick={() => onChange({
                      ...draft,
                      headers: draft.headers.filter((_, itemIndex) => itemIndex !== index),
                    })}
                  >
                    <Trash2 size={15} aria-hidden="true" />
                  </button>
                </div>
              ))}
            </div>
          </div>
        )}

        {draft.auth_mode !== "none" && credential?.environment_override && (
          <small className="api-environment-note">{t("settings.apiManagement.environmentManaged")}</small>
        )}
      </div>
      <div className="api-profile-editor-actions">
        <div>
          {credential?.stored_configured && !credential.environment_override && onRemoveCredential && (
            <button className="secondary-button api-danger-button" type="button" disabled={saving} onClick={onRemoveCredential}>
              {t("settings.apiManagement.removeCredential")}
            </button>
          )}
        </div>
        <div className="settings-inline-actions">
          {onCancel && <button className="secondary-button" type="button" disabled={saving} onClick={onCancel}>{t("common.cancel")}</button>}
          <button className="primary-button" type="submit" disabled={!canSave}>{t("common.save")}</button>
        </div>
      </div>
    </form>
  );
}

function ConnectionField({
  field,
  value,
  disabled,
  floating,
  onChange,
}: {
  field: ProviderConnectionField;
  value: string | number | boolean;
  disabled: boolean;
  floating?: "dialog";
  onChange: (value: string | number | boolean) => void;
}) {
  const { t } = useTranslation();
  const label = field.label ?? (field.label_key ? t(field.label_key) : t(`settings.apiManagement.connectionFields.${field.id}`));
  if (field.type === "select" || field.options?.length) {
    return (
      <Select
        label={label}
        value={String(value)}
        options={(field.options ?? []).map((option) => ({ value: option.value, label: option.label }))}
        disabled={disabled}
        floating={floating}
        onChange={onChange}
      />
    );
  }
  if (field.type === "boolean") {
    return (
      <div className={`field api-profile-local-field ${disabled ? "disabled" : ""}`}>
        <span>{label}</span>
        <div className="api-profile-local-control">
          <button
            className="settings-switch-button"
            type="button"
            role="switch"
            aria-checked={Boolean(value)}
            aria-label={label}
            disabled={disabled}
            onClick={() => onChange(!value)}
          >
            <span className="switch-track" aria-hidden="true"><span /></span>
          </button>
        </div>
      </div>
    );
  }
  return (
    <label className="field cloud-text-field">
      <span>{label}</span>
      <input
        type={field.type === "number" ? "number" : "text"}
        value={String(value)}
        min={field.min}
        max={field.max}
        step={field.step}
        required={field.required}
        disabled={disabled}
        spellCheck={false}
        placeholder={field.placeholder}
        onChange={(event) => onChange(field.type === "number" ? Number(event.target.value) : event.target.value)}
      />
    </label>
  );
}
