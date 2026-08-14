import type { ApiProfile, ApiProfilePurpose, ApiProfileView, ApiProvider } from "../../types";
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
    };
  }
  return { name: draft.name.trim(), provider: draft.provider, purpose: "llm" };
}

export function ApiProfileEditor({
  draft,
  saving,
  credential,
  providers = ["alibaba_cloud", "openai", "openai_compatible", "deepl", "microsoft_translator"],
  purposes = ["asr", "llm", "shared"],
  requireCredential = false,
  onChange,
  onSave,
  onCancel,
  onRemoveCredential,
}: {
  draft: ApiProfileEditorDraft;
  saving: boolean;
  credential?: ApiProfileView["credential"];
  providers?: ApiProvider[];
  purposes?: ApiProfilePurpose[];
  requireCredential?: boolean;
  onChange: (draft: ApiProfileEditorDraft) => void;
  onSave: () => void;
  onCancel?: () => void;
  onRemoveCredential?: () => void;
}) {
  const { t } = useTranslation();
  const editing = Boolean(draft.id);
  const credentialAvailable = credential?.configured || Boolean(draft.api_key.trim());
  const workspaceRequired = draft.provider === "alibaba_cloud" && draft.purpose !== "llm";
  const canSave = Boolean(draft.name.trim())
    && (draft.provider !== "openai_compatible" || Boolean(draft.base_url.trim()))
    && (draft.provider !== "microsoft_translator" || Boolean(draft.region.trim()))
    && (!workspaceRequired || Boolean(draft.workspace_id.trim()))
    && (!requireCredential || credentialAvailable)
    && !saving;

  const providerOptions = providers.map((provider) => ({
    value: provider,
    label: provider === "alibaba_cloud"
      ? "Alibaba Cloud"
      : provider === "openai"
        ? t("settings.apiManagement.openaiProvider")
        : provider === "openai_compatible"
          ? t("settings.apiManagement.openaiCompatibleProvider")
        : provider === "deepl"
          ? "DeepL"
          : "Microsoft Translator",
  }));

  return (
    <form className="api-profile-editor" onSubmit={(event) => { event.preventDefault(); onSave(); }}>
      <div className="api-profile-editor-heading">
        <strong>{t(editing ? "settings.apiManagement.editProfile" : "settings.apiManagement.addProfile")}</strong>
        <small>{t("settings.apiManagement.profileFormHint")}</small>
      </div>
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
          disabled={editing || saving}
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
          <label className="field cloud-text-field">
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
        )}
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
      </div>
      {credential?.environment_override && (
        <small className="api-environment-note">{t("settings.apiManagement.environmentManaged")}</small>
      )}
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
