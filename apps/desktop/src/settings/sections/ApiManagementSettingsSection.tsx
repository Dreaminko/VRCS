import { Check, Cloud, KeyRound, Pencil, Plus, RefreshCw, ShieldCheck, Trash2 } from "lucide-react";
import { Fragment, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  apiProfilePurpose,
  supportsLlmModels,
  supportsRecognition,
  supportsTranslation,
} from "../../api-profile-purpose";
import type { ApiProfileView, ApiProvider, ProviderDefinition, Settings } from "../../types";
import {
  ApiProfileEditor,
  apiProfileFromEditorDraft,
  createApiProfileDraft,
  type ApiProfileEditorDraft,
} from "../api/ApiProfileEditor";
import { useApiProfiles } from "../useApiProfiles";

function draftFromProfile(profile: ApiProfileView): ApiProfileEditorDraft {
  return {
    id: profile.id,
    name: profile.name,
    provider: profile.provider,
    purpose: apiProfilePurpose(profile),
    region: profile.region ?? "china_beijing",
    workspace_id: profile.workspace_id ?? "",
    base_url: profile.base_url ?? "",
    api_key: "",
    preset_id: profile.preset_id ?? "custom",
    auth_mode: profile.auth_mode ?? "bearer",
    is_local: profile.is_local ?? false,
    timeout_ms: profile.timeout_ms ?? 8000,
    headers: profile.headers ?? [],
  };
}

function providerLabel(provider: ApiProvider, definitions: ProviderDefinition[]) {
  const definition = definitions.find((item) => item.id === provider);
  if (definition) return definition.display_name;
  if (provider === "alibaba_cloud") return "Alibaba Cloud";
  if (provider === "microsoft_translator") return "Microsoft Translator";
  if (provider === "deepl") return "DeepL";
  if (provider === "openai_compatible") return "OpenAI Compatible";
  if (provider === "gemini") return "Gemini";
  return "OpenAI";
}

function supportLevel(profile: ApiProfileView) {
  const levels = [profile.support_levels.asr, profile.support_levels.translation].filter(Boolean);
  if (new Set(levels).size > 1) return "mixed";
  return levels[0] ?? null;
}

export function ApiManagementSettingsSection({
  settings,
  disabled,
  onRefreshSettings,
}: {
  settings: Settings;
  disabled: boolean;
  onRefreshSettings: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const profiles = useApiProfiles(onRefreshSettings);
  const [editor, setEditor] = useState<ApiProfileEditorDraft | null>(null);
  const locked = disabled || profiles.busy !== null;

  const saveEditor = async () => {
    if (!editor?.name.trim()) return;
    const profile = apiProfileFromEditorDraft(editor);
    const saved = editor.id
      ? await profiles.update({ id: editor.id, ...profile }, editor.api_key)
      : await profiles.create(profile, editor.api_key);
    if (saved) setEditor(null);
  };

  const removeProfile = async (profile: ApiProfileView) => {
    const affectsTranslation = profile.translation_active;
    const impact = profile.active && affectsTranslation
      ? t("settings.apiManagement.deleteImpacts.transcriptionAndTranslation")
      : profile.active
        ? t("settings.apiManagement.deleteImpacts.transcription")
        : affectsTranslation
          ? t("settings.apiManagement.deleteImpacts.translation")
          : "";
    if (!window.confirm(t("settings.apiManagement.confirmDelete", {
      name: profile.name,
      impact,
    }))) return;
    if (await profiles.remove(profile.id)) setEditor((current) => current?.id === profile.id ? null : current);
  };

  return (
    <div className="settings-section settings-section-active api-section" id="settings-panel-api" role="tabpanel" aria-labelledby="settings-tab-api">
      <div className="section-heading api-section-heading">
        <div><KeyRound size={18} /><h2>{t("settings.apiManagement.title")}</h2></div>
        <button className="primary-button api-add-button" type="button" disabled={locked || Boolean(editor)} onClick={() => setEditor(createApiProfileDraft())}>
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
          <ApiProfileEditor
            draft={editor}
            saving={profiles.busy === "create"}
            providerDefinitions={profiles.providerDefinitions}
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
          const recognitionReady = recognitionCapable && !(
            profile.provider === "alibaba_cloud" && !profile.workspace_id?.trim()
          );
          const translationCapable = supportsTranslation(profile);
          const supportsModels = supportsLlmModels(profile);
          const purpose = apiProfilePurpose(profile);
          const profileSupportLevel = supportLevel(profile);
          const modelCatalog = profiles.modelCatalogs[profile.id];
          const diagnostic = profiles.diagnostics[profile.id];
          const credentialReady = !profile.capabilities.requires_api_key
            || profile.credential.configured;
          const transcriptionActive = profile.active && (
            (profile.provider === "alibaba_cloud" && ["qwen_realtime", "fun_asr_realtime"].includes(settings.asr.backend))
            || (profile.provider === "openai" && settings.asr.backend === "openai_realtime")
          );
          const translationActive = profile.translation_active && (
            settings.translation.mode !== "disabled"
            || settings.translation.translate_microphone
          );
          const detail = profile.provider === "alibaba_cloud"
            ? `${profile.region === "singapore" ? "Singapore" : "China (Beijing)"} · ${profile.workspace_id || t("settings.apiManagement.workspaceMissing")}`
            : profile.provider === "microsoft_translator"
              ? profile.region
              : profile.provider === "openai_compatible"
                ? profile.base_url
                : profile.provider === "gemini"
                  ? t("settings.apiManagement.geminiDescription")
                : t(`settings.apiManagement.${profile.provider === "deepl" ? "deeplDescription" : "openaiDescription"}`);
          return (
            <Fragment key={profile.id}>
              <section className={`api-profile-row ${profile.active ? "active" : ""}`} aria-label={profile.name}>
                <div className="api-profile-identity">
                  <span className="api-profile-icon"><Cloud size={16} aria-hidden="true" /></span>
                  <span>
                    <strong>{profile.name}</strong>
                    <small>{providerLabel(profile.provider, profiles.providerDefinitions)} · {t(`settings.apiManagement.purposes.${purpose}`)} · {profileSupportLevel ? t(`settings.apiManagement.supportLevels.${profileSupportLevel}`) : ""} · {detail}</small>
                    {supportsModels && modelCatalog && (
                      <small className={modelCatalog.error ? "api-model-catalog-error" : ""}>
                        {modelCatalog.loading
                          ? t("settings.apiManagement.loadingModels")
                          : modelCatalog.error
                            ? modelCatalog.error
                            : modelCatalog.models.length
                              ? t("settings.apiManagement.modelsAvailable", { count: modelCatalog.models.length })
                              : t("settings.apiManagement.modelsUnavailable")}
                      </small>
                    )}
                    {diagnostic?.checks?.map((check) => (
                      <small className={check.status === "failed" ? "api-model-catalog-error" : ""} key={check.name}>
                        {t(`settings.apiManagement.diagnostics.checks.${check.name}`)}: {t(`settings.apiManagement.diagnostics.status.${check.status}`)}
                        {check.code ? ` · ${t(`settings.apiManagement.diagnostics.codes.${check.code.replaceAll(".", "_")}`, { defaultValue: check.detail ?? check.code })}` : ""}
                      </small>
                    ))}
                  </span>
                </div>
                <div className="api-profile-actions">
                  {profile.active ? (
                    <span className="api-active-badge"><Check size={13} aria-hidden="true" />{t(transcriptionActive
                      ? "settings.apiManagement.transcriptionActive"
                      : "settings.apiManagement.transcriptionDefault")}</span>
                  ) : recognitionCapable ? (
                    <button className="secondary-button" type="button" disabled={locked || !profile.credential.configured || !recognitionReady} onClick={() => void profiles.activate(profile.provider === "alibaba_cloud" ? "alibaba_cloud" : "openai", profile.id)}>{t("settings.apiManagement.setDefault")}</button>
                  ) : null}
                  {translationActive && <span className="api-active-badge"><Check size={13} aria-hidden="true" />{t("settings.apiManagement.translationActive")}</span>}
                  {supportsModels && (
                    <button
                      className="secondary-button"
                      type="button"
                      disabled={locked || !credentialReady || modelCatalog?.loading}
                      onClick={() => void profiles.refreshModels(profile.id)}
                    >
                      <RefreshCw className={modelCatalog?.loading ? "spin" : ""} size={14} />
                      {t("settings.apiManagement.refreshModels")}
                    </button>
                  )}
                  {recognitionCapable && <button className="secondary-button" type="button" disabled={profiles.busy !== null || !credentialReady || !recognitionReady} onClick={() => void profiles.test(profile.id, "asr")}>{t("settings.apiManagement.testAsr")}</button>}
                  {translationCapable && <button className="secondary-button" type="button" disabled={profiles.busy !== null || !credentialReady} onClick={() => void profiles.test(profile.id, "llm", undefined, settings.translation.profile_id === profile.id ? settings.translation.model : undefined)}>{t("settings.apiManagement.testTranslation")}</button>}
                  <button className="api-row-icon-button" type="button" aria-label={t("common.edit")} disabled={locked || Boolean(editor)} onClick={() => setEditor(draftFromProfile(profile))}><Pencil size={15} /></button>
                  <button className="api-row-icon-button danger" type="button" aria-label={t("common.delete")} disabled={locked} onClick={() => void removeProfile(profile)}><Trash2 size={15} /></button>
                </div>
              </section>
              {editing && editor && (
                <ApiProfileEditor
                  draft={editor}
                  saving={profiles.busy === profile.id}
                  credential={profile.credential}
                  providerDefinitions={profiles.providerDefinitions}
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
