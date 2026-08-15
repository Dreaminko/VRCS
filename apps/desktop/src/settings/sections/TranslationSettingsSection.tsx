import { Cloud, Languages, Mic2, RefreshCw, Workflow } from "lucide-react";
import { useTranslation } from "react-i18next";

import { supportsContext, supportsLlmModels, supportsTranslation } from "../../api-profile-purpose";
import { EditableDropdownField } from "../../components/DropdownField";
import { LanguagePicker } from "../../components/LanguagePicker";
import { TRANSLATION_LANGUAGE_CODES } from "../../translation-languages";
import type { ApiProfileView, Settings, TranslationSettings } from "../../types";
import { useTranslationProfileModels } from "../hooks/useTranslationProfileModels";
import type { ApplySettings, SaveState } from "../settings-types";
import { PreferenceToggle, Select } from "../SettingsControls";
import { TranslationEnhancementSettings } from "../translation/TranslationEnhancementSettings";

function modelForProfile(profile: ApiProfileView | undefined, current: string): string {
  if (profile?.provider === "alibaba_cloud") return "qwen-plus";
  if (profile?.provider === "openai") return "gpt-5-mini";
  return current;
}

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
  const supportsThinkingToggle = Boolean(
    selectedProfile?.provider === "openai_compatible"
      && selectedProfile.preset_id === "deepseek",
  );
  const {
    models: availableModels,
    loading: modelsLoading,
    error: modelsError,
    refresh: refreshModels,
  } = useTranslationProfileModels(selectedProfile, usesLlmProfile);
  const update = (translation: TranslationSettings) => applySettings((current) => ({
    ...current,
    translation,
  }));
  const controlsDisabled = saveState === "saving";

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
                const profileId = mode === "disabled"
                  ? draft.translation.profile_id
                  : selectedProfile?.id ?? translationProfiles[0]?.id ?? null;
                const profile = translationProfiles.find((item) => item.id === profileId);
                update({
                  ...draft.translation,
                  mode: mode as TranslationSettings["mode"],
                  profile_id: profileId,
                  model: modelForProfile(profile, draft.translation.model),
                });
              }}
            />
          </div>
        </div>

        <div className="translation-config-row">
          <div className="translation-config-title">
            <Mic2 size={17} />
            <span>
              <strong>{t("settings.translation.ownVoice")}</strong>
            </span>
          </div>
          <div className="translation-own-voice-fields">
            <PreferenceToggle
              title={t("settings.translation.translateOwnVoice")}
              checked={draft.translation.translate_microphone}
              disabled={controlsDisabled || !translationProfiles.length}
              onChange={(translate_microphone) => {
                const profileId = translate_microphone
                  ? selectedProfile?.id ?? translationProfiles[0]?.id ?? null
                  : draft.translation.profile_id;
                const profile = translationProfiles.find((item) => item.id === profileId);
                update({
                  ...draft.translation,
                  translate_microphone,
                  profile_id: profileId,
                  model: modelForProfile(profile, draft.translation.model),
                });
              }}
            />
            <LanguagePicker
              label={t("settings.translation.ownVoiceTargetLanguage")}
              helper={translationProfiles.length ? undefined : t("settings.translation.noProfiles")}
              value={draft.translation.microphone_target_language}
              disabled={controlsDisabled || !draft.translation.translate_microphone}
              languageCodes={languageCodes}
              allowCustom={allowCustomLanguage}
              onChange={(microphone_target_language) => update({
                ...draft.translation,
                microphone_target_language,
              })}
            />
          </div>
        </div>

        <div className="translation-config-row">
          <div className="translation-config-title">
            <Languages size={17} />
            <span>
              <strong>{t("settings.translation.targetLanguage")}</strong>
            </span>
          </div>
          <div className="translation-config-fields translation-config-fields-single">
            <LanguagePicker
              label={t("settings.translation.targetLanguage")}
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
              helper={translationProfiles.length ? undefined : t("settings.translation.noProfiles")}
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
              onChange={(profile_id) => {
                const profile = translationProfiles.find((item) => item.id === profile_id);
                update({
                  ...draft.translation,
                  profile_id,
                  model: modelForProfile(profile, draft.translation.model),
                });
              }}
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
                {(modelsLoading || modelsError || availableModels.length > 0) && (
                  <small className={modelsError ? "api-model-catalog-error" : ""}>
                    {modelsLoading
                      ? t("settings.apiManagement.loadingModels")
                      : modelsError || t("settings.apiManagement.modelsAvailable", { count: availableModels.length })}
                  </small>
                )}
                {supportsThinkingToggle && (
                  <div className="translation-thinking-toggle">
                    <span>
                      <strong>{t("settings.translation.thinkingMode")}</strong>
                    </span>
                    <button
                      className="settings-switch-button"
                      type="button"
                      role="switch"
                      aria-checked={draft.translation.thinking_enabled}
                      aria-label={t("settings.translation.thinkingMode")}
                      disabled={controlsDisabled}
                      onClick={() => update({
                        ...draft.translation,
                        thinking_enabled: !draft.translation.thinking_enabled,
                      })}
                    >
                      <span className="switch-track" aria-hidden="true"><span /></span>
                    </button>
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
