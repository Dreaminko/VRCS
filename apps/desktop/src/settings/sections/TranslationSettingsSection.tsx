import { Cloud, Languages, RefreshCw, Workflow } from "lucide-react";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { supportsContext, supportsLlmModels, supportsTranslation } from "../../api-profile-purpose";
import { localizedError } from "../../app/app-utils";
import { EditableDropdownField } from "../../shared/ui/DropdownField";
import { LanguagePicker } from "../../shared/ui/LanguagePicker";
import { TRANSLATION_LANGUAGE_CODES } from "../../translation-languages";
import { selectTranslationModel } from "../../translation-model-selection";
import { thinkingControlForModel } from "../../translation-thinking";
import type { ApiProfileView, Settings, TranslationSettings } from "../../types";
import { useTranslationProfileModels } from "../hooks/useTranslationProfileModels";
import type { ApplySettings, SaveState } from "../settings-types";
import { PreferenceToggle, Select } from "../SettingsControls";
import { TranslationEnhancementSettings } from "../translation/TranslationEnhancementSettings";


function profileOptionLabel(profile: ApiProfileView): string {
  const provider = profile.provider_display_name;
  return profile.name.localeCompare(provider, undefined, { sensitivity: "base" }) === 0
    ? profile.name
    : `${profile.name} · ${provider}`;
}

export function TranslationSettingsSection({ draft, apiProfiles, saveState, applySettings }: {
  draft: Settings;
  apiProfiles: ApiProfileView[];
  saveState: SaveState;
  applySettings: ApplySettings;
}) {
  const { t } = useTranslation();
  const [profileSwitching, setProfileSwitching] = useState(false);
  const [profileSwitchError, setProfileSwitchError] = useState("");
  const profileRequest = useRef(0);
  const translationProfiles = apiProfiles.filter(supportsTranslation);
  const selectedProfile = translationProfiles.find(
    (profile) => profile.id === draft.translation.profile_id,
  );
  const usesLlmProfile = Boolean(selectedProfile && supportsLlmModels(selectedProfile));
  const usesContextProfile = Boolean(selectedProfile && supportsContext(selectedProfile));
  const languageCodes = selectedProfile?.capabilities.supported_languages
    ?? TRANSLATION_LANGUAGE_CODES;
  const allowCustomLanguage = selectedProfile?.capabilities.supports_custom_translation_language
    ?? false;
  const thinkingControl = thinkingControlForModel(
    selectedProfile?.provider,
    draft.translation.model,
  );
  const {
    models: availableModels,
    loading: modelsLoading,
    error: modelsError,
    refresh: refreshModels,
    load: loadModels,
  } = useTranslationProfileModels(selectedProfile, usesLlmProfile);
  const update = (translation: TranslationSettings) => applySettings((current) => ({
    ...current,
    translation,
  }));
  const controlsDisabled = saveState === "saving" || profileSwitching;
  const selectProfile = async (
    profileId: string,
    mode = draft.translation.mode,
  ) => {
    const profile = translationProfiles.find((item) => item.id === profileId);
    if (!profile) return;
    const currentRequest = ++profileRequest.current;
    setProfileSwitching(true);
    setProfileSwitchError("");
    try {
      let model = draft.translation.model;
      if (supportsLlmModels(profile)) {
        if (profile.provider === "openai_compatible") {
          model = draft.translation.model.trim();
        } else {
          const models = await loadModels(profile.id);
          model = selectTranslationModel(profile.provider, models, draft.translation.model) ?? "";
        }
        if (!model) throw { code: "llm.model_required" };
      }
      if (currentRequest !== profileRequest.current) return;
      update({ ...draft.translation, mode, profile_id: profile.id, model });
    } catch (reason) {
      if (currentRequest === profileRequest.current) {
        setProfileSwitchError(localizedError(reason, t, "errors.apiProfiles.models"));
      }
    } finally {
      if (currentRequest === profileRequest.current) setProfileSwitching(false);
    }
  };

  return (
    <div className="settings-section settings-section-active translation-section" id="settings-panel-translation" role="tabpanel" aria-labelledby="settings-tab-translation">
      <div className="section-heading">
        <div>
          <Languages size={18} />
          <h2>{t("settings.translation.title")}</h2>
        </div>
      </div>

      <div className="translation-config">
        <div className="translation-config-row">
          <div className="translation-config-title">
            <Workflow size={17} />
            <span>
              <strong>{t("settings.translation.mode")}</strong>
            </span>
          </div>
          <div className="translation-config-fields translation-config-fields-single">
            <Select
              label={t("settings.translation.mode")}
              value={draft.translation.mode}
              disabled={controlsDisabled || !translationProfiles.length}
              helper={!translationProfiles.length ? t("settings.translation.noProfiles") : undefined}
              options={["disabled", "manual", "automatic"].map((value) => ({
                value,
                label: t(`settings.translation.modes.${value}`),
              }))}
              onChange={(mode) => {
                const nextMode = mode as TranslationSettings["mode"];
                if (nextMode === "disabled") {
                  update({ ...draft.translation, mode: nextMode });
                  return;
                }
                const profile = selectedProfile ?? translationProfiles[0];
                if (profile) void selectProfile(profile.id, nextMode);
              }}
            />
          </div>
        </div>

        <div className="translation-config-row">
          <div className="translation-config-title">
            <Languages size={17} />
            <span>
              <strong>{t("settings.translation.targetLanguageSettings")}</strong>
            </span>
          </div>
          <div className="translation-config-fields">
            <LanguagePicker
              label={t("settings.translation.targetLanguageForSelf")}
              helper={translationProfiles.length ? undefined : t("settings.translation.noProfiles")}
              value={draft.translation.microphone_target_language}
              disabled={controlsDisabled || draft.translation.mode !== "automatic" || !translationProfiles.length}
              languageCodes={languageCodes}
              allowCustom={allowCustomLanguage}
              onChange={(microphone_target_language) => update({
                ...draft.translation,
                microphone_target_language,
              })}
            />
            <LanguagePicker
              label={t("settings.translation.targetLanguageForOtherParty")}
              value={draft.translation.target_language}
              disabled={controlsDisabled}
              languageCodes={languageCodes}
              allowCustom={allowCustomLanguage}
              onChange={(target_language) => update({
                ...draft.translation,
                target_language,
              })}
            />
          </div>
        </div>

        <div className="translation-config-row">
          <div className="translation-config-title">
            <Cloud size={17} />
            <span>
              <strong>{t("settings.translation.profile")}</strong>
            </span>
          </div>
          <div className={`translation-config-fields ${usesLlmProfile ? "" : "translation-config-fields-single"}`}>
            <Select
              label={t("settings.translation.profile")}
              helper={profileSwitchError || (translationProfiles.length ? undefined : t("settings.translation.noProfiles"))}
              value={selectedProfile?.id ?? ""}
              disabled={controlsDisabled || !translationProfiles.length}
              options={[
                ...(!draft.translation.profile_id
                  ? [{ value: "", label: t("settings.translation.selectProfile") }]
                  : []),
                ...translationProfiles.map((profile) => ({
                  value: profile.id,
                  label: profileOptionLabel(profile),
                })),
              ]}
              onChange={(profile_id) => void selectProfile(profile_id)}
            />
            {usesLlmProfile && (
              <div className="field translation-model-field">
                <span>{t("settings.translation.model")}</span>
                <div className="translation-model-input-row">
                  <EditableDropdownField
                    label={t("settings.translation.model")}
                    value={draft.translation.model}
                    options={availableModels.map((model) => ({ value: model, label: model }))}
                    disabled={controlsDisabled}
                    optionsDisabled={modelsLoading || !availableModels.length}
                    placeholder={t("settings.translation.manualModel")}
                    onChange={(model) => update({ ...draft.translation, model })}
                  />
                  <button
                    className="secondary-button"
                    type="button"
                    disabled={modelsLoading}
                    onClick={() => void refreshModels()}
                  >
                    <RefreshCw className={modelsLoading ? "spin" : ""} size={14} />
                    {t("common.refresh")}
                  </button>
                </div>
                {(profileSwitching || modelsLoading || profileSwitchError || modelsError || availableModels.length > 0) && (
                  <small className={profileSwitchError || modelsError ? "api-model-catalog-error" : ""}>
                    {profileSwitching || modelsLoading
                      ? t("settings.apiManagement.loadingModels")
                      : profileSwitchError || modelsError || t("settings.apiManagement.modelsAvailable", { count: availableModels.length })}
                  </small>
                )}
                {thinkingControl === "disable_supported" && (
                  <PreferenceToggle
                    title={t("settings.translation.thinkingMode")}
                    description={t("settings.translation.thinkingModeDescription")}
                    checked={draft.translation.thinking_enabled}
                    disabled={controlsDisabled}
                    onChange={(thinking_enabled) => update({
                      ...draft.translation,
                      thinking_enabled,
                    })}
                  />
                )}
                {thinkingControl === "hide_only" && (
                  <div className="settings-toggle-row translation-thinking-status">
                    <span className="settings-toggle-copy">
                      <strong>{t("settings.translation.thinkingMode")}</strong>
                      <small>{t("settings.translation.thinkingModeAlwaysOnDescription")}</small>
                    </span>
                    <span className="status-chip active">
                      {t("settings.translation.thinkingModeAlwaysOn")}
                    </span>
                  </div>
                )}
              </div>
            )}
          </div>
        </div>

        {usesContextProfile && selectedProfile && (
          <TranslationEnhancementSettings
            translation={draft.translation}
            profile={selectedProfile}
            disabled={controlsDisabled}
            onChange={(patch) => update({
              ...draft.translation,
              prompt: { ...draft.translation.prompt, ...patch },
            })}
            onGlossarySourcesChange={(glossary_sources, afterSave, afterError) => applySettings((current) => ({
              ...current,
              translation: {
                ...current.translation,
                prompt: { ...current.translation.prompt, glossary_sources },
              },
            }), afterSave, afterError)}
          />
        )}
      </div>
    </div>
  );
}
