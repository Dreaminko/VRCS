import { Check, Cloud, KeyRound, Pencil, Plus, RefreshCw, ShieldCheck, Trash2 } from "lucide-react";
import { Fragment, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  apiProfilePurpose,
  supportsLlmModels,
  supportsRecognition,
  supportsTranslation,
} from "../../api-profile-purpose";
import type { ApiProfile, ApiProfilePurpose, ApiProfileView, ApiProvider } from "../../types";
import { Select } from "../SettingsControls";
import { useApiProfiles } from "../useApiProfiles";

interface EditorDraft {
  id?: string;
  name: string;
  provider: ApiProvider;
  purpose: ApiProfilePurpose;
  region: string;
  workspace_id: string;
  base_url: string;
  api_key: string;
}

const emptyDraft = (): EditorDraft => ({
  name: "",
  provider: "alibaba_cloud",
  purpose: "shared",
  region: "china_beijing",
  workspace_id: "",
  base_url: "",
  api_key: "",
});

function draftFromProfile(profile: ApiProfileView): EditorDraft {
  return {
    id: profile.id,
    name: profile.name,
    provider: profile.provider,
    purpose: apiProfilePurpose(profile),
    region: profile.region ?? "china_beijing",
    workspace_id: profile.workspace_id ?? "",
    base_url: profile.base_url ?? "",
    api_key: "",
  };
}

function ProfileEditor({
  draft,
  saving,
  credential,
  onChange,
  onSave,
  onCancel,
  onRemoveCredential,
}: {
  draft: EditorDraft;
  saving: boolean;
  credential?: ApiProfileView["credential"];
  onChange: (draft: EditorDraft) => void;
  onSave: () => void;
  onCancel: () => void;
  onRemoveCredential?: () => void;
}) {
  const { t } = useTranslation();
  const editing = Boolean(draft.id);
  const canSave = Boolean(draft.name.trim())
    && (draft.provider !== "microsoft_translator" || Boolean(draft.region.trim()))
    && !saving;

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
          options={[
            { value: "alibaba_cloud", label: "Alibaba Cloud" },
            { value: "openai", label: "OpenAI / Compatible" },
            { value: "deepl", label: "DeepL" },
            { value: "microsoft_translator", label: "Microsoft Translator" },
          ]}
          disabled={editing || saving}
          onChange={(value) => {
            const provider = value as ApiProvider;
            onChange({
              ...draft,
              provider,
              purpose: provider === "openai" ? "asr" : provider === "alibaba_cloud" ? "shared" : "llm",
              region: provider === "microsoft_translator"
                ? "eastasia"
                : provider === "alibaba_cloud"
                  ? "china_beijing"
                  : "",
              workspace_id: "",
              base_url: "",
            });
          }}
        />
        {(draft.provider === "alibaba_cloud" || draft.provider === "openai") && (
          <Select
            label={t("settings.apiManagement.purpose")}
            value={draft.purpose}
            options={[
              { value: "asr", label: t("settings.apiManagement.purposes.asr") },
              { value: "llm", label: t("settings.apiManagement.purposes.llm") },
              { value: "shared", label: t("settings.apiManagement.purposes.shared") },
            ]}
            disabled={saving}
            onChange={(value) => onChange({
              ...draft,
              purpose: value as ApiProfilePurpose,
              base_url: value === "llm" ? draft.base_url : "",
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
            onChange={(value) => onChange({ ...draft, region: value as EditorDraft["region"] })}
          />
          <label className="field cloud-text-field">
            <span>Workspace ID</span>
            <input
              value={draft.workspace_id}
              disabled={saving}
              spellCheck={false}
              onChange={(event) => onChange({ ...draft, workspace_id: event.target.value })}
            />
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
        {draft.provider === "openai" && draft.purpose === "llm" && (
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
          <button className="secondary-button" type="button" disabled={saving} onClick={onCancel}>{t("common.cancel")}</button>
          <button className="primary-button" type="submit" disabled={!canSave}>{t("common.save")}</button>
        </div>
      </div>
    </form>
  );
}

function providerLabel(provider: ApiProvider) {
  if (provider === "alibaba_cloud") return "Alibaba Cloud";
  if (provider === "microsoft_translator") return "Microsoft Translator";
  if (provider === "deepl") return "DeepL";
  return "OpenAI";
}

export function ApiManagementSettingsSection({
  disabled,
  onRefreshSettings,
}: {
  disabled: boolean;
  onRefreshSettings: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const profiles = useApiProfiles(onRefreshSettings);
  const [editor, setEditor] = useState<EditorDraft | null>(null);
  const locked = disabled || profiles.busy !== null;

  const saveEditor = async () => {
    if (!editor?.name.trim()) return;
    const profile: Omit<ApiProfile, "id"> = editor.provider === "alibaba_cloud"
      ? {
          name: editor.name.trim(),
          provider: editor.provider,
          region: editor.region,
          workspace_id: editor.workspace_id.trim(),
          purpose: editor.purpose,
        }
      : editor.provider === "microsoft_translator"
        ? { name: editor.name.trim(), provider: editor.provider, region: editor.region.trim(), purpose: "llm" }
        : editor.provider === "openai"
          ? {
              name: editor.name.trim(),
              provider: editor.provider,
              base_url: editor.purpose === "llm" ? editor.base_url.trim() || undefined : undefined,
              purpose: editor.purpose,
            }
          : { name: editor.name.trim(), provider: editor.provider, purpose: "llm" };
    const saved = editor.id
      ? await profiles.update({ id: editor.id, ...profile }, editor.api_key)
      : await profiles.create(profile, editor.api_key);
    if (saved) setEditor(null);
  };

  const removeProfile = async (profile: ApiProfileView) => {
    if (!window.confirm(t("settings.apiManagement.confirmDelete", { name: profile.name }))) return;
    if (await profiles.remove(profile.id)) setEditor((current) => current?.id === profile.id ? null : current);
  };

  return (
    <div className="settings-section settings-section-active api-section" id="settings-panel-api" role="tabpanel" aria-labelledby="settings-tab-api">
      <div className="section-heading api-section-heading">
        <div><KeyRound size={18} /><h2>{t("settings.apiManagement.title")}</h2></div>
        <button className="primary-button api-add-button" type="button" disabled={locked || Boolean(editor)} onClick={() => setEditor(emptyDraft())}>
          <Plus size={18} aria-hidden="true" />
          {t("settings.apiManagement.addProfile")}
        </button>
      </div>
      <p className="api-section-subtitle">{t("settings.apiManagement.subtitle")}</p>
      <div className="api-security-note">
        <ShieldCheck size={18} aria-hidden="true" />
        <p>{t("settings.apiManagement.securityNotice")}</p>
      </div>

      <div className="api-profile-list" aria-busy={profiles.loading || undefined}>
        {editor && !editor.id && (
          <ProfileEditor
            draft={editor}
            saving={profiles.busy === "create"}
            onChange={setEditor}
            onSave={() => void saveEditor()}
            onCancel={() => setEditor(null)}
          />
        )}
        {profiles.loading && <p className="api-profile-empty">{t("settings.apiManagement.checking")}</p>}
        {!profiles.loading && !profiles.profiles.length && !editor && (
          <div className="api-profile-empty">
            <Cloud size={20} aria-hidden="true" />
            <strong>{t("settings.apiManagement.emptyTitle")}</strong>
            <small>{t("settings.apiManagement.emptyDescription")}</small>
          </div>
        )}
        {profiles.profiles.map((profile) => {
          const editing = editor?.id === profile.id;
          const recognitionCapable = supportsRecognition(profile);
          const translationCapable = supportsTranslation(profile);
          const supportsModels = supportsLlmModels(profile);
          const purpose = apiProfilePurpose(profile);
          const modelCatalog = profiles.modelCatalogs[profile.id];
          const status = profile.credential.environment_override
            ? t("settings.apiManagement.sourceEnvironment")
            : profile.credential.configured
              ? t("settings.apiManagement.configured")
              : t("settings.apiManagement.notConfigured");
          const detail = profile.provider === "alibaba_cloud"
            ? `${profile.region === "singapore" ? "Singapore" : "China (Beijing)"} · ${profile.workspace_id || t("settings.apiManagement.workspaceMissing")}`
            : profile.provider === "microsoft_translator"
              ? profile.region
              : profile.provider === "openai" && profile.base_url
                ? profile.base_url
                : t(`settings.apiManagement.${profile.provider === "deepl" ? "deeplDescription" : "openaiDescription"}`);
          return (
            <Fragment key={profile.id}>
              <section className={`api-profile-row ${profile.active ? "active" : ""}`} aria-label={profile.name}>
                <div className="api-profile-identity">
                  <span className="api-profile-icon"><Cloud size={16} aria-hidden="true" /></span>
                  <span>
                    <strong>{profile.name}</strong>
                    <small>{providerLabel(profile.provider)} · {t(`settings.apiManagement.purposes.${purpose}`)} · {detail}</small>
                    {supportsModels && modelCatalog && (
                      <small className={modelCatalog.error ? "api-model-catalog-error" : ""}>
                        {modelCatalog.loading
                          ? t("settings.apiManagement.loadingModels")
                          : modelCatalog.error
                            ? modelCatalog.error
                            : modelCatalog.models.length
                              ? `${t("settings.apiManagement.modelsAvailable", { count: modelCatalog.models.length })} · ${modelCatalog.models.slice(0, 4).join(", ")}${modelCatalog.models.length > 4 ? ` +${modelCatalog.models.length - 4}` : ""}`
                              : t("settings.apiManagement.modelsUnavailable")}
                      </small>
                    )}
                  </span>
                </div>
                <div className="api-profile-status">
                  <span className={`api-status-dot ${profile.credential.configured ? "configured" : ""}`} aria-hidden="true" />
                  <span><strong>{status}</strong>{profile.credential.stored_configured && profile.credential.environment_override && <small>{t("settings.apiManagement.storedCredentialAvailable")}</small>}</span>
                </div>
                <div className="api-profile-actions">
                  {profile.active ? (
                    <span className="api-active-badge"><Check size={13} aria-hidden="true" />{t("settings.apiManagement.transcriptionActive")}</span>
                  ) : recognitionCapable ? (
                    <button className="secondary-button" type="button" disabled={locked || !profile.credential.configured} onClick={() => void profiles.activate(profile.provider === "alibaba_cloud" ? "alibaba_cloud" : "openai", profile.id)}>{t("settings.apiManagement.setActive")}</button>
                  ) : null}
                  {profile.translation_active && <span className="api-active-badge"><Check size={13} aria-hidden="true" />{t("settings.apiManagement.translationActive")}</span>}
                  {supportsModels && (
                    <button
                      className="secondary-button"
                      type="button"
                      disabled={locked || !profile.credential.configured || modelCatalog?.loading}
                      onClick={() => void profiles.refreshModels(profile.id)}
                    >
                      <RefreshCw className={modelCatalog?.loading ? "spin" : ""} size={14} />
                      {t("settings.apiManagement.refreshModels")}
                    </button>
                  )}
                  {recognitionCapable && <button className="secondary-button" type="button" disabled={profiles.busy !== null || !profile.credential.configured} onClick={() => void profiles.test(profile.id, "asr")}>{t("settings.apiManagement.testAsr")}</button>}
                  {translationCapable && <button className="secondary-button" type="button" disabled={profiles.busy !== null || !profile.credential.configured} onClick={() => void profiles.test(profile.id, "llm")}>{t("settings.apiManagement.testTranslation")}</button>}
                  <button className="api-row-icon-button" type="button" aria-label={t("common.edit")} disabled={locked || Boolean(editor)} onClick={() => setEditor(draftFromProfile(profile))}><Pencil size={15} /></button>
                  <button className="api-row-icon-button danger" type="button" aria-label={t("common.delete")} disabled={locked} onClick={() => void removeProfile(profile)}><Trash2 size={15} /></button>
                </div>
              </section>
              {editing && editor && (
                <ProfileEditor
                  draft={editor}
                  saving={profiles.busy === profile.id}
                  credential={profile.credential}
                  onChange={setEditor}
                  onSave={() => void saveEditor()}
                  onCancel={() => setEditor(null)}
                  onRemoveCredential={() => void profiles.removeCredential(profile.id)}
                />
              )}
            </Fragment>
          );
        })}
      </div>
      {disabled && <small className="api-credential-message">{t("settings.recognition.stopToModify")}</small>}
      {profiles.message && <small className="api-credential-message" role="status">{profiles.message}</small>}
    </div>
  );
}
