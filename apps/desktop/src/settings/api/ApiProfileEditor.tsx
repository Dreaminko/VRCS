import type {
  ApiAuthMode,
  ApiProfile,
  ApiProfilePurpose,
  ApiProfileView,
  ApiProvider,
  HttpHeaderConfig,
  ProviderDefinition,
} from "../../types";
import { Plus, Trash2 } from "lucide-react";
import { Select } from "../SettingsControls";
import { useTranslation } from "react-i18next";

export interface ApiProfileEditorDraft {
  id?: string;
  name: string;
  provider: ApiProvider;
  purpose: ApiProfilePurpose;
  region: string;
  workspace_id: string;
  base_url: string;
  api_key: string;
  preset_id: string;
  auth_mode: ApiAuthMode;
  is_local: boolean;
  timeout_ms: number;
  headers: HttpHeaderConfig[];
}

export function createApiProfileDraft(provider: ApiProvider = "alibaba_cloud"): ApiProfileEditorDraft {
  return {
    name: "",
    provider,
    purpose: provider === "openai" ? "asr" : provider === "alibaba_cloud" ? "shared" : "llm",
    region: provider === "alibaba_cloud" ? "china_beijing" : provider === "microsoft_translator" ? "eastasia" : "",
    workspace_id: "",
    base_url: "",
    api_key: "",
    preset_id: "custom",
    auth_mode: "bearer",
    is_local: false,
    timeout_ms: 8000,
    headers: [],
  };
}

export function apiProfileFromEditorDraft(draft: ApiProfileEditorDraft): Omit<ApiProfile, "id"> {
  if (draft.provider === "alibaba_cloud") {
    return {
      name: draft.name.trim(),
      provider: draft.provider,
      region: draft.region,
      workspace_id: draft.workspace_id.trim(),
      purpose: draft.purpose,
    };
  }
  if (draft.provider === "microsoft_translator") {
    return {
      name: draft.name.trim(),
      provider: draft.provider,
      region: draft.region.trim(),
      purpose: "llm",
    };
  }
  if (draft.provider === "openai") {
    return {
      name: draft.name.trim(),
      provider: draft.provider,
      purpose: draft.purpose,
    };
  }
  if (draft.provider === "openai_compatible") {
    return {
      name: draft.name.trim(),
      provider: draft.provider,
      base_url: draft.base_url.trim(),
      purpose: "llm",
      preset_id: draft.preset_id,
      auth_mode: draft.auth_mode,
      is_local: draft.is_local,
      timeout_ms: draft.timeout_ms,
      headers: draft.headers
        .map((header) => ({ name: header.name.trim(), value: header.value }))
        .filter((header) => header.name),
    };
  }
  return { name: draft.name.trim(), provider: draft.provider, purpose: "llm" };
}

export function ApiProfileEditor({
  draft,
  saving,
  credential,
  providers = ["alibaba_cloud", "openai", "gemini", "openai_compatible", "deepl", "microsoft_translator"],
  providerDefinitions = [],
  purposes = ["asr", "llm", "shared"],
  requireCredential = false,
  floatingSelects = false,
  onChange,
  onSave,
  onCancel,
  onRemoveCredential,
}: {
  draft: ApiProfileEditorDraft;
  saving: boolean;
  credential?: ApiProfileView["credential"];
  providers?: ApiProvider[];
  providerDefinitions?: ProviderDefinition[];
  purposes?: ApiProfilePurpose[];
  requireCredential?: boolean;
  floatingSelects?: boolean;
  onChange: (draft: ApiProfileEditorDraft) => void;
  onSave: () => void;
  onCancel?: () => void;
  onRemoveCredential?: () => void;
}) {
  const { t } = useTranslation();
  const editing = Boolean(draft.id);
  const nameOptional = !editing && draft.purpose === "llm";
  const credentialAvailable = credential?.configured || Boolean(draft.api_key.trim());
  const compatibleDefinition = providerDefinitions.find(
    (definition) => definition.id === "openai_compatible",
  );
  const workspaceRequired = draft.provider === "alibaba_cloud" && draft.purpose !== "llm";
  const localServiceDisabled = saving || draft.preset_id !== "custom";
  const canSave = (nameOptional || Boolean(draft.name.trim()))
    && (draft.provider !== "openai_compatible" || Boolean(draft.base_url.trim()))
    && (draft.provider !== "microsoft_translator" || Boolean(draft.region.trim()))
    && (!workspaceRequired || Boolean(draft.workspace_id.trim()))
    && (!requireCredential || draft.auth_mode === "none" || credentialAvailable)
    && draft.timeout_ms >= 1000
    && draft.timeout_ms <= 120000
    && !saving;

  const providerOptions = providers.map((provider) => ({
    value: provider,
    label: providerDefinitions.find((definition) => definition.id === provider)?.display_name ?? (provider === "alibaba_cloud"
      ? "Alibaba Cloud"
      : provider === "openai"
        ? t("settings.apiManagement.openaiProvider")
        : provider === "openai_compatible"
          ? t("settings.apiManagement.openaiCompatibleProvider")
        : provider === "gemini"
          ? "Gemini"
        : provider === "deepl"
          ? "DeepL"
          : "Microsoft Translator"),
  }));

  const apiKeyField = draft.auth_mode !== "none" && (
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
        onChange={(event) => onChange({ ...draft, api_key: event.target.value })}
      />
    </label>
  );

  return (
    <form className="api-profile-editor" onSubmit={(event) => { event.preventDefault(); onSave(); }}>
      <div className="api-profile-editor-heading">
        <strong>{t(editing ? "settings.apiManagement.editProfile" : "settings.apiManagement.addProfile")}</strong>
        <small>{t("settings.apiManagement.profileFormHint")}</small>
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
            {nameOptional && <small>{t("settings.apiManagement.profileNameOptionalHint")}</small>}
          </label>
          <Select
            label={t("settings.apiManagement.provider")}
            value={draft.provider}
            options={providerOptions}
            disabled={editing || saving}
            floating={floatingSelects ? "dialog" : undefined}
            onChange={(value) => {
              const provider = value as ApiProvider;
              onChange({ ...createApiProfileDraft(provider), name: draft.name });
            }}
          />
          {(draft.provider === "alibaba_cloud" || draft.provider === "openai") && (
            <Select
              label={t("settings.apiManagement.purpose")}
              value={draft.purpose}
              options={purposes.map((purpose) => ({
                value: purpose,
                label: t(`settings.apiManagement.purposes.${purpose}`),
              }))}
              disabled={saving}
              floating={floatingSelects ? "dialog" : undefined}
              onChange={(value) => onChange({
                ...draft,
                purpose: value as ApiProfilePurpose,
                base_url: "",
              })}
            />
          )}
          {draft.provider === "alibaba_cloud" && <>
            <Select
              label={t("settings.apiManagement.region")}
              value={draft.region}
              options={[
                { value: "china_beijing", label: "China (Beijing)" },
                { value: "singapore", label: "Singapore" },
              ]}
              disabled={saving}
              floating={floatingSelects ? "dialog" : undefined}
              onChange={(value) => onChange({ ...draft, region: value })}
            />
            <label className="field cloud-text-field">
              <span>{t("settings.apiManagement.workspaceId")}</span>
              <input
                value={draft.workspace_id}
                disabled={saving}
                spellCheck={false}
                aria-invalid={workspaceRequired && !draft.workspace_id.trim()}
                onChange={(event) => onChange({ ...draft, workspace_id: event.target.value })}
              />
              <small>{t(workspaceRequired
                ? "settings.apiManagement.workspaceIdRequired"
                : "settings.apiManagement.workspaceIdOptional")}</small>
            </label>
          </>}
          {draft.provider === "microsoft_translator" && (
            <label className="field cloud-text-field">
              <span>{t("settings.apiManagement.region")}</span>
              <input
                value={draft.region}
                disabled={saving}
                placeholder="eastasia"
                onChange={(event) => onChange({ ...draft, region: event.target.value })}
              />
            </label>
          )}
          {draft.provider === "openai_compatible" && (
            <section className="api-compatible-settings" aria-label={t("settings.apiManagement.openaiCompatibleProvider")}>
              <div className="api-compatible-grid">
                <Select
                  label={t("settings.apiManagement.preset")}
                  value={draft.preset_id}
                  options={(compatibleDefinition?.presets ?? []).map((preset) => ({
                    value: preset.id,
                    label: preset.display_name,
                  }))}
                  disabled={saving}
                  floating={floatingSelects ? "dialog" : undefined}
                  onChange={(presetId) => {
                    const preset = compatibleDefinition?.presets.find((item) => item.id === presetId);
                    onChange({
                      ...draft,
                      preset_id: presetId,
                      base_url: preset?.base_url || draft.base_url,
                      auth_mode: preset?.auth_mode ?? draft.auth_mode,
                      is_local: preset?.is_local ?? draft.is_local,
                    });
                  }}
                />
                <Select
                  label={t("settings.apiManagement.authentication")}
                  value={draft.auth_mode}
                  options={[
                    { value: "bearer", label: t("settings.apiManagement.authBearer") },
                    { value: "none", label: t("settings.apiManagement.authNone") },
                  ]}
                  disabled={saving || draft.preset_id !== "custom"}
                  floating={floatingSelects ? "dialog" : undefined}
                  onChange={(value) => onChange({ ...draft, auth_mode: value as ApiAuthMode })}
                />
                <label className="field cloud-text-field api-compatible-url-field">
                  <span>{t("settings.apiManagement.baseUrl")}</span>
                  <input
                    type="url"
                    value={draft.base_url}
                    disabled={saving}
                    spellCheck={false}
                    placeholder="https://api.deepseek.com/v1"
                    onChange={(event) => onChange({ ...draft, base_url: event.target.value })}
                  />
                  <small>{t("settings.apiManagement.baseUrlHint")}</small>
                </label>
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
                <div className={`field api-profile-local-field ${localServiceDisabled ? "disabled" : ""}`}>
                  <span>{t("settings.apiManagement.localService")}</span>
                  <div className="api-profile-local-control">
                    <small>{t("settings.apiManagement.localServiceHint")}</small>
                    <button
                      className="settings-switch-button"
                      type="button"
                      role="switch"
                      aria-checked={draft.is_local}
                      aria-label={t("settings.apiManagement.localService")}
                      disabled={localServiceDisabled}
                      onClick={() => onChange({ ...draft, is_local: !draft.is_local })}
                    >
                      <span className="switch-track" aria-hidden="true"><span /></span>
                    </button>
                  </div>
                </div>
              </div>
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
              {apiKeyField}
            </section>
          )}
          {draft.provider !== "openai_compatible" && apiKeyField}
        </div>
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
