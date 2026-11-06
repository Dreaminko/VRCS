import { Check, Cloud, FlaskConical, KeyRound, Pencil, Plus, RefreshCw, Square, Trash2 } from "lucide-react";
import { useRef, useState, type ReactNode, type RefObject } from "react";
import { useTranslation } from "react-i18next";

import { supportsLlmModels } from "../../providers/api-profile-purpose";
import {
  apiProfileDraftFromView,
  apiProfileFromEditorDraft,
  createApiProfileDraft,
  type ApiProfileEditorDraft,
} from "../../providers/api-profile-draft";
import { profileEnabledCapabilities, providerDefinition, providerDetail } from "../../providers/provider-catalog";
import { providerIcon } from "../../providers/provider-icon";
import { recognitionServicesForProfile, selectRecognitionProfile } from "../../providers/recognition-services";
import { localizedLanguageName } from "../../translation/translation-languages";
import type {
  ApiCapability,
  ApiProfileView,
  ApiProfileUsage,
  ProviderDefinition,
} from "../../providers/types";
import type { Settings } from "../types";
import { ConfirmDialog } from "../../app/components/ConfirmDialog";
import { ApiProfileEditor } from "../api/ApiProfileEditor";
import type { ApiCapabilityTests } from "../api/ApiCapabilitySelector";
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
  error,
  errorField,
  warning,
  capabilityTests,
  capabilityFeedback,
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
  error: string;
  errorField?: string;
  warning: string;
  capabilityTests?: ApiCapabilityTests;
  capabilityFeedback?: ReactNode;
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
        error={error}
        errorField={errorField}
        warning={warning}
        capabilityTests={capabilityTests}
        capabilityFeedback={capabilityFeedback}
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
  embedded = false,
}: {
  settings: Settings;
  onRefreshSettings: () => Promise<void>;
  embedded?: boolean;
}) {
  const { t, i18n } = useTranslation();
  const profiles = useApiProfiles(onRefreshSettings);
  const [editor, setEditor] = useState<ApiProfileEditorDraft | null>(null);
  const [testingCapability, setTestingCapability] = useState<ApiCapability | null>(null);
  const [pendingRemoval, setPendingRemoval] = useState<{
    profile: ApiProfileView;
    message: string;
  } | null>(null);
  const [pendingUpdate, setPendingUpdate] = useState<{ draft: ApiProfileEditorDraft; message: string } | null>(null);
  const [removing, setRemoving] = useState(false);
  const editorTriggerRef = useRef<HTMLButtonElement>(null);
  const removalTriggerRef = useRef<HTMLButtonElement>(null);
  const locked = profiles.isBusy("create");
  const editingProfile = editor?.id
    ? profiles.profiles.find((profile) => profile.id === editor.id)
    : undefined;
  const testableCapabilities = editingProfile ? profileEnabledCapabilities(editingProfile).filter(
    (capability) => capability !== "speech_to_text"
      || recognitionServicesForProfile(editingProfile, profiles.providerDefinitions).length > 0,
  ) : [];
  const testBusy = Boolean(editingProfile && profiles.isBusy(editingProfile.id));
  const testRequiresSave = Boolean(editingProfile && editor && (editor.api_key.trim()
    || JSON.stringify(apiProfileFromEditorDraft(editor)) !== JSON.stringify(apiProfileFromEditorDraft(
      apiProfileDraftFromView(editingProfile, profiles.providerDefinitions),
    ))));
  const credentialReadyForTest = editingProfile && (!editingProfile.capabilities.requires_api_key || editingProfile.credential.configured);
  const editorModels = editingProfile ? profiles.modelCatalogs[editingProfile.id] : undefined;
  const capabilityTests: ApiCapabilityTests | undefined = editingProfile && editor
    ? Object.fromEntries(testableCapabilities.map((capability) => {
      const label = capability === "speech_to_text" ? "testAsr" : capability === "text_translation" ? "testTranslation"
        : capability === "image_understanding" ? "testImage" : "testText";
      const testing = profiles.isTesting(editingProfile.id) && testingCapability === capability;
      const actionLabel = t(`settings.apiManagement.${testing ? "cancelTest" : label}`);
      return [capability,
        <button className="api-row-icon-button api-profile-test-button" type="button"
          aria-label={actionLabel}
          data-tooltip={actionLabel}
          disabled={!testing && (testBusy || testRequiresSave || !credentialReadyForTest
            || !editor.enabled_capabilities.includes(capability))}
          onClick={() => {
            if (testing) { profiles.cancelTest(editingProfile.id); return; }
            setTestingCapability(capability);
            void profiles.test(editingProfile.id, capability,
              capability === "speech_to_text" ? selectRecognitionProfile(
                settings.asr, editingProfile.id, profiles.profiles, profiles.providerDefinitions,
              ).backend : undefined);
          }}
        >{testing ? <Square size={15} aria-hidden="true" /> : <FlaskConical size={15} aria-hidden="true" />}</button>,
      ];
    })) : undefined;
  const usageLabel = (usage: ApiProfileUsage) => {
    const details = [
      usage.preset_name,
      usage.scope === "model_selection" ? t("settings.modelAssignments.models")
        : usage.scope ? t(`settings.translation.contextSources.${usage.scope}`) : "",
      usage.index !== undefined ? `#${usage.index + 1}` : "",
      usage.target_language ? localizedLanguageName(usage.target_language, i18n.language) : "",
    ].filter(Boolean).join(" · ");
    return t(`settings.apiManagement.usages.${usage.kind}`) + (details ? ` (${details})` : "");
  };
  const affectedFeatures = (editingProfile?.usages ?? [])
    .filter((usage) => editor && !editor.enabled_capabilities.includes(usage.capability))
    .map(usageLabel).join(", ");

  const commitEditor = async (draft: ApiProfileEditorDraft) => {
    const profile = apiProfileFromEditorDraft(draft);
    const saved = draft.id
      ? await profiles.update({ id: draft.id, ...profile }, draft.api_key)
      : await profiles.create(profile, draft.api_key);
    if (saved) setEditor(null);
  };

  const saveEditor = async () => {
    if (!editor) return;
    if (editor.id) {
      const preview = await profiles.previewUpdate({ id: editor.id, ...apiProfileFromEditorDraft(editor) }, editor.api_key);
      if (!preview) return;
      if (preview.side_effects.length) {
        setPendingUpdate({
          draft: editor,
          message: t("settings.apiManagement.affectedFeatures", { features: preview.side_effects.map(usageLabel).join(", ") }),
        });
        return;
      }
    }
    await commitEditor(editor);
  };

  const confirmUpdate = async () => {
    if (!pendingUpdate) return;
    try {
      await commitEditor(pendingUpdate.draft);
    } finally {
      setPendingUpdate(null);
    }
  };

  const requestRemoval = async (profile: ApiProfileView, button: HTMLButtonElement) => {
    removalTriggerRef.current = button;
    const preview = await profiles.previewDeletion(profile.id);
    if (!preview) return;
    const features = preview.side_effects.map(usageLabel).join(", ");
    const impact = features ? t("settings.apiManagement.affectedFeatures", { features }) : "";
    setPendingRemoval({
      profile,
      message: t("settings.apiManagement.confirmDelete", { name: profile.name, impact }),
    });
  };

  const confirmRemoval = async () => {
    if (!pendingRemoval || removing) return;
    setRemoving(true);
    try {
      if (await profiles.remove(pendingRemoval.profile.id)) {
        setEditor((current) => current?.id === pendingRemoval.profile.id ? null : current);
      }
    } finally {
      setRemoving(false);
      setPendingRemoval(null);
    }
  };

  return (
    <div className="settings-section settings-section-active api-section" id={embedded ? undefined : "settings-panel-api"} role={embedded ? undefined : "tabpanel"} aria-labelledby={embedded ? undefined : "settings-tab-api"}>
      <div className="section-heading">
        <div><KeyRound size={18} /><h2>{t("settings.apiManagement.title")}</h2></div>
        <button
          className="primary-button api-add-button"
          type="button"
          disabled={locked || Boolean(editor)}
          onClick={(event) => {
            editorTriggerRef.current = event.currentTarget;
            profiles.clearEditorError();
            setEditor(createApiProfileDraft(profiles.providerDefinitions));
          }}
        >
          <Plus size={18} aria-hidden="true" />
          {t("settings.apiManagement.addProfile")}
        </button>
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
          const locked = profiles.isBusy(profile.id);
          const unavailable = Boolean(profile.status && profile.status !== "ready");
          const recognitionServices = recognitionServicesForProfile(profile, profiles.providerDefinitions);
          const supportsModels = supportsLlmModels(profile);
          const profileSupportLevel = supportLevel(profile);
          const modelCatalog = profiles.modelCatalogs[profile.id];
          const credentialReady = !profile.capabilities.requires_api_key
            || profile.credential.configured;
          const transcriptionActive = profile.active ?? (settings.asr.active_profile_id === profile.id
            && recognitionServices.some((service) => service.id === settings.asr.backend));
          const translationActive = profile.translation_active
            && settings.translation.mode !== "disabled";
          const detail = providerDetail(profile, definition);
          const Logo = providerIcon(profile);
          const initial = (definition?.display_name ?? profile.provider_display_name ?? profile.provider)
            .trim()
            .charAt(0)
            .toUpperCase();
          const capabilityLabels = profileEnabledCapabilities(profile).map(
            (capability) => t(`settings.apiManagement.capabilities.${capability}`),
          ).join(", ");
          return (
            <section className={`api-profile-row ${transcriptionActive || translationActive ? "active" : ""}`} aria-label={profile.name} key={profile.id}>
                <div className="api-profile-identity">
                  <span className="api-profile-icon">
                    {Logo ? (
                      <Logo size={18} aria-hidden="true" />
                    ) : (
                      <strong aria-hidden="true">{initial}</strong>
                    )}
                  </span>
                  <span>
                    <strong>{profile.name}</strong>
                    <small>{definition?.display_name ?? profile.provider_display_name ?? profile.provider} · {capabilityLabels}{profileSupportLevel ? ` · ${t(`settings.apiManagement.supportLevels.${profileSupportLevel}`)}` : ""}{detail ? ` · ${detail}` : ""}</small>
                    {unavailable && <small className="api-model-catalog-error">{t(`settings.apiManagement.profileStatus.${profile.status}`)}</small>}
                    {profiles.rowMessages[profile.id] && <small role="status">{profiles.rowMessages[profile.id]}</small>}
                    {modelCatalog?.error && <small className="api-model-catalog-error">{modelCatalog.error}</small>}
                  </span>
                </div>
                <div className="api-profile-actions">
                  {!unavailable && <>
                  {transcriptionActive && (
                    <span className="api-active-badge"><Check size={13} aria-hidden="true" />{t("settings.apiManagement.transcriptionActive")}</span>
                  )}
                  {translationActive && <span className="api-active-badge"><Check size={13} aria-hidden="true" />{t("settings.apiManagement.translationActive")}</span>}
                  {supportsModels && (
                    <button
                      className="api-row-icon-button"
                      type="button"
                      aria-label={t("settings.apiManagement.refreshModels")}
                      data-tooltip={t("settings.apiManagement.refreshModels")}
                      disabled={locked || !credentialReady || modelCatalog?.loading}
                      onClick={() => void profiles.refreshModels(profile.id)}
                    >
                      <RefreshCw className={modelCatalog?.loading ? "spin" : ""} size={15} aria-hidden="true" />
                    </button>
                  )}
                  <button
                    className="api-row-icon-button"
                    type="button"
                    aria-label={t("common.edit")}
                    disabled={locked || Boolean(editor)}
                    onClick={(event) => {
                      editorTriggerRef.current = event.currentTarget;
                      profiles.clearEditorError();
                      setEditor(apiProfileDraftFromView(profile, profiles.providerDefinitions));
                    }}
                  >
                    <Pencil size={15} />
                  </button>
                  </>}
                  <button className="api-row-icon-button danger" type="button" aria-label={t("common.delete")} disabled={locked} onClick={(event) => void requestRemoval(profile, event.currentTarget)}><Trash2 size={15} /></button>
                </div>
            </section>
          );
        })}
      </div>
      {editor && (
        <ApiProfileEditorDialog
          draft={editor}
          saving={profiles.isBusy(editor.id ?? "create")}
          error={profiles.editorError}
          errorField={profiles.editorErrorField}
          warning={affectedFeatures ? t("settings.apiManagement.affectedFeatures", { features: affectedFeatures }) : ""}
          capabilityTests={capabilityTests}
          capabilityFeedback={editingProfile && (
            <div className="api-profile-tests">
              {testRequiresSave && <small className="api-environment-note">{t("settings.apiManagement.saveBeforeTest")}</small>}
              {profiles.rowMessages[editingProfile.id] && <small className="api-environment-note" role="status">{profiles.rowMessages[editingProfile.id]}</small>}
              {editorModels?.error && <small className="api-model-catalog-error">{editorModels.error}</small>}
            </div>
          )}
          credential={editingProfile?.credential}
          providerDefinitions={profiles.providerDefinitions}
          returnFocusRef={editorTriggerRef}
          onChange={(draft) => { profiles.clearEditorError(); setEditor(draft); }}
          onSave={() => void saveEditor()}
          onClose={() => setEditor(null)}
          onRemoveCredential={editingProfile
            ? () => void profiles.removeCredential(editingProfile.id)
            : undefined}
        />
      )}
      {pendingRemoval && (
        <ConfirmDialog
          title={t("common.delete")}
          message={pendingRemoval.message}
          confirmLabel={t("common.delete")}
          busy={removing}
          icon={<Trash2 size={16} />}
          returnFocusRef={removalTriggerRef}
          onClose={() => setPendingRemoval(null)}
          onConfirm={confirmRemoval}
        />
      )}
      {pendingUpdate && (
        <ConfirmDialog
          title={t("settings.apiManagement.editProfile")}
          message={pendingUpdate.message}
          confirmLabel={t("common.save")}
          busy={profiles.isBusy(pendingUpdate.draft.id ?? "create")}
          onClose={() => setPendingUpdate(null)}
          onConfirm={confirmUpdate}
        />
      )}
      {profiles.message && <small className="api-credential-message" role="status">{profiles.message}</small>}
    </div>
  );
}
