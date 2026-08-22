import { Check, Cloud, KeyRound, Pencil, Plus, RefreshCw, ShieldCheck, Trash2 } from "lucide-react";
import { useRef, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";

import {
  supportsLlmModels,
  supportsRecognition,
  supportsTranslation,
} from "../../api-profile-purpose";
import {
  apiProfileDraftFromView,
  apiProfileFromEditorDraft,
  createApiProfileDraft,
  type ApiProfileEditorDraft,
} from "../../api-profile-draft";
import { profileEnabledCapabilities, providerDefinition, providerDetail } from "../../provider-catalog";
import { recognitionServicesForProfile } from "../../recognition-services";
import { translationDiagnosticModel } from "../../translation-model-selection";
import type {
  ApiProfileView,
  ProviderDefinition,
} from "../../providers/types";
import type { Settings } from "../types";
import { ApiProfileEditor } from "../api/ApiProfileEditor";
import { SettingsDialog } from "../components/SettingsDialog";
import { useApiProfiles } from "../useApiProfiles";



function supportLevel(profile: ApiProfileView) {
  const levels = [profile.support_levels.asr, profile.support_levels.translation].filter(Boolean);
  if (new Set(levels).size > 1) return "mixed";
  return levels[0] ?? null;
}

function ApiProfileEditorDialog({
  draft,
  saving,
  credential,
  providerDefinitions,
  returnFocusRef,
  onChange,
  onSave,
  onClose,
  onRemoveCredential,
}: {
  draft: ApiProfileEditorDraft;
  saving: boolean;
  credential?: ApiProfileView["credential"];
  providerDefinitions: ProviderDefinition[];
  returnFocusRef: RefObject<HTMLButtonElement | null>;
  onChange: (draft: ApiProfileEditorDraft) => void;
  onSave: () => void;
  onClose: () => void;
  onRemoveCredential?: () => void;
}) {
  const { t } = useTranslation();
  return (
    <SettingsDialog
      label={t(draft.id ? "settings.apiManagement.editProfile" : "settings.apiManagement.addProfile")}
      saving={saving}
      returnFocusRef={returnFocusRef}
      onClose={onClose}
    >
      <ApiProfileEditor
        draft={draft}
        saving={saving}
        credential={credential}
        providerDefinitions={providerDefinitions}
        floatingSelects
        onChange={onChange}
        onSave={onSave}
        onCancel={onClose}
        onRemoveCredential={onRemoveCredential}
      />
    </SettingsDialog>
  );
}

export function ApiManagementSettingsSection({
  settings,
  onRefreshSettings,
}: {
  settings: Settings;
  onRefreshSettings: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const profiles = useApiProfiles(onRefreshSettings);
  const [editor, setEditor] = useState<ApiProfileEditorDraft | null>(null);
  const editorTriggerRef = useRef<HTMLButtonElement>(null);
  const locked = profiles.busy !== null;
  const editingProfile = editor?.id
    ? profiles.profiles.find((profile) => profile.id === editor.id)
    : undefined;

  const saveEditor = async () => {
    if (!editor || !editor.name.trim()) return;
    const profile = apiProfileFromEditorDraft(editor);
    const saved = editor.id
      ? await profiles.update({ id: editor.id, ...profile }, editor.api_key)
      : await profiles.create(profile, editor.api_key);
    if (saved) setEditor(null);
  };

  const removeProfile = async (profile: ApiProfileView) => {
    const affectsTranscription = settings.asr.active_profile_id === profile.id;
    const affectsTranslation = profile.translation_active;
    const impact = affectsTranscription && affectsTranslation
      ? t("settings.apiManagement.deleteImpacts.transcriptionAndTranslation")
      : affectsTranscription
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
      <div className="section-heading">
        <div><KeyRound size={18} /><h2>{t("settings.apiManagement.title")}</h2></div>
        <button
          className="primary-button api-add-button"
          type="button"
          disabled={locked || Boolean(editor)}
          onClick={(event) => {
            editorTriggerRef.current = event.currentTarget;
            setEditor(createApiProfileDraft(profiles.providerDefinitions));
          }}
        >
          <Plus size={18} aria-hidden="true" />
          {t("settings.apiManagement.addProfile")}
        </button>
      </div>
      <div className="api-security-note">
        <ShieldCheck size={18} aria-hidden="true" />
        <p>{t("settings.apiManagement.securityNotice")}</p>
      </div>

      <div className="api-profile-list" aria-busy={profiles.loading || undefined}>
        {profiles.loading && <p className="api-profile-empty">{t("settings.apiManagement.checking")}</p>}
        {!profiles.loading && !profiles.profiles.length && (
          <div className="api-profile-empty">
            <Cloud size={20} aria-hidden="true" />
            <strong>{t("settings.apiManagement.emptyTitle")}</strong>
            <small>{t("settings.apiManagement.emptyDescription")}</small>
          </div>
        )}
        {profiles.profiles.map((profile) => {
          const definition = providerDefinition(profiles.providerDefinitions, profile.provider);
          const recognitionServices = recognitionServicesForProfile(profile, profiles.providerDefinitions);
          const recognitionCapable = supportsRecognition(profile) && recognitionServices.length > 0;
          const translationCapable = supportsTranslation(profile);
          const supportsModels = supportsLlmModels(profile);
          const profileSupportLevel = supportLevel(profile);
          const modelCatalog = profiles.modelCatalogs[profile.id];
          const diagnostic = profiles.diagnostics[profile.id];
          const credentialReady = !profile.capabilities.requires_api_key
            || profile.credential.configured;
          const transcriptionActive = settings.asr.active_profile_id === profile.id
            && recognitionServices.some((service) => service.id === settings.asr.backend);
          const translationActive = profile.translation_active
            && settings.translation.mode !== "disabled";
          const translationTestModel = translationActive
            ? translationDiagnosticModel(
              profile,
              modelCatalog,
              settings.translation.speaker_targets.find(
                (target) => target.profile_id === profile.id,
              )?.model ?? "",
            )
            : undefined;
          const detail = providerDetail(profile, definition);
          const capabilityLabels = profileEnabledCapabilities(profile).map(
            (capability) => t(`settings.apiManagement.capabilities.${capability}`),
          ).join(", ");
          return (
            <section className={`api-profile-row ${transcriptionActive || translationActive ? "active" : ""}`} aria-label={profile.name} key={profile.id}>
                <div className="api-profile-identity">
                  <span className="api-profile-icon"><Cloud size={16} aria-hidden="true" /></span>
                  <span>
                    <strong>{profile.name}</strong>
                    <small>{definition?.display_name ?? profile.provider_display_name ?? profile.provider} · {capabilityLabels}{profileSupportLevel ? ` · ${t(`settings.apiManagement.supportLevels.${profileSupportLevel}`)}` : ""}{detail ? ` · ${detail}` : ""}</small>
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
                  {transcriptionActive ? (
                    <span className="api-active-badge"><Check size={13} aria-hidden="true" />{t("settings.apiManagement.transcriptionActive")}</span>
                  ) : recognitionCapable ? (
                    <button className="secondary-button" type="button" disabled={locked || !credentialReady} onClick={() => void profiles.activate(profile.id, recognitionServices[0].id)}>{t("settings.apiManagement.setDefault")}</button>
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
                  {recognitionCapable && <button className="secondary-button" type="button" disabled={profiles.busy !== null || !credentialReady} onClick={() => void profiles.test(profile.id, "speech_to_text", recognitionServices[0].id)}>{t("settings.apiManagement.testAsr")}</button>}
                  {translationCapable && <button className="secondary-button" type="button" disabled={profiles.busy !== null || !credentialReady} onClick={() => void profiles.test(profile.id, "text_translation", undefined, translationTestModel)}>{t("settings.apiManagement.testTranslation")}</button>}
                  <button
                    className="api-row-icon-button"
                    type="button"
                    aria-label={t("common.edit")}
                    disabled={locked || Boolean(editor)}
                    onClick={(event) => {
                      editorTriggerRef.current = event.currentTarget;
                      setEditor(apiProfileDraftFromView(profile, profiles.providerDefinitions));
                    }}
                  >
                    <Pencil size={15} />
                  </button>
                  <button className="api-row-icon-button danger" type="button" aria-label={t("common.delete")} disabled={locked} onClick={() => void removeProfile(profile)}><Trash2 size={15} /></button>
                </div>
            </section>
          );
        })}
      </div>
      {editor && (
        <ApiProfileEditorDialog
          draft={editor}
          saving={profiles.busy === (editor.id ?? "create")}
          credential={editingProfile?.credential}
          providerDefinitions={profiles.providerDefinitions}
          returnFocusRef={editorTriggerRef}
          onChange={setEditor}
          onSave={() => void saveEditor()}
          onClose={() => setEditor(null)}
          onRemoveCredential={editingProfile
            ? () => void profiles.removeCredential(editingProfile.id)
            : undefined}
        />
      )}
      {profiles.message && <small className="api-credential-message" role="status">{profiles.message}</small>}
    </div>
  );
}
