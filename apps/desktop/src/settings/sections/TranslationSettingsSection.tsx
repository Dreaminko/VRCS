import { Cloud, Languages, Mic2, RefreshCw, Workflow } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { coreApi } from "../../api";
import { localizedError } from "../../app-utils";
import { EditableDropdownField } from "../../components/DropdownField";
import type { ApiProfile, Settings, TranslationSettings } from "../../types";
import type { ApplySettings, SaveState } from "../settings-types";
import { PreferenceToggle, Select } from "../SettingsControls";

const targetLanguages: TranslationSettings["target_language"][] = [
  "zh-Hans", "zh-Hant", "en", "ja", "ko", "es", "fr", "de", "ru",
];

function modelForProfile(profile: ApiProfile | undefined, current: string): string {
  if (profile?.provider === "alibaba_cloud") return "qwen-plus";
  if (profile?.provider === "openai" && !profile.base_url) return "gpt-5-mini";
  return current;
}

export function TranslationSettingsSection({ draft, disabled, saveState, applySettings }: {
  draft: Settings;
  disabled: boolean;
  saveState: SaveState;
  applySettings: ApplySettings;
}) {
  const { t } = useTranslation();
  const selectedProfile = draft.asr.api_profiles.find(
    (profile) => profile.id === draft.translation.profile_id,
  );
  const usesLlmProfile = Boolean(
    selectedProfile && ["openai", "alibaba_cloud"].includes(selectedProfile.provider),
  );
  const supportsThinkingToggle = Boolean(
    selectedProfile?.provider === "openai"
      && (
        selectedProfile.base_url?.toLowerCase().includes("deepseek")
        || draft.translation.model.toLowerCase().startsWith("deepseek-")
      ),
  );
  const [availableModels, setAvailableModels] = useState<string[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelsError, setModelsError] = useState("");
  const modelRequest = useRef(0);
  const refreshModels = useCallback(async () => {
    if (!selectedProfile || !usesLlmProfile) {
      setAvailableModels([]);
      setModelsError("");
      return;
    }
    const request = ++modelRequest.current;
    setModelsLoading(true);
    setModelsError("");
    try {
      const response = await coreApi.apiProfileModels(selectedProfile.id);
      if (request === modelRequest.current) setAvailableModels(response.models);
    } catch (reason) {
      if (request === modelRequest.current) {
        setAvailableModels([]);
        setModelsError(localizedError(reason, t, "errors.apiProfiles.models"));
      }
    } finally {
      if (request === modelRequest.current) setModelsLoading(false);
    }
  }, [selectedProfile?.id, t, usesLlmProfile]);
  useEffect(() => {
    void refreshModels();
    return () => { modelRequest.current += 1; };
  }, [refreshModels]);
  const update = (translation: TranslationSettings) => applySettings((current) => ({
    ...current,
    translation,
  }));
  const controlsDisabled = disabled || saveState === "saving";
  const statusMode = draft.translation.mode === "disabled" && draft.translation.translate_microphone
    ? "microphone"
    : draft.translation.mode;

  return (
    <div className="settings-section settings-section-active translation-section" id="settings-panel-translation" role="tabpanel" aria-labelledby="settings-tab-translation">
      <div className="section-heading translation-section-heading">
        <div>
          <Languages size={18} />
          <h2>{t("settings.translation.title")}</h2>
          <span className={`status-chip ${statusMode}`}>
            {t(`settings.translation.modes.${statusMode}`)}
          </span>
        </div>
      </div>
      <p className="translation-section-subtitle">{t("settings.translation.description")}</p>

      <div className="translation-config">
        <div className="translation-config-row">
          <div className="translation-config-title">
            <Workflow size={17} />
            <span>
              <strong>{t("settings.translation.mode")}</strong>
              <small>{t("settings.translation.modeDescription")}</small>
            </span>
          </div>
          <div className="translation-config-fields translation-config-fields-single">
            <Select
              label={t("settings.translation.mode")}
              value={draft.translation.mode}
              disabled={controlsDisabled || !draft.asr.api_profiles.length}
              helper={!draft.asr.api_profiles.length ? t("settings.translation.noProfiles") : undefined}
              options={["disabled", "manual", "automatic"].map((value) => ({
                value,
                label: t(`settings.translation.modes.${value}`),
              }))}
              onChange={(mode) => {
                const profileId = mode === "disabled"
                  ? draft.translation.profile_id
                  : draft.translation.profile_id ?? draft.asr.api_profiles[0]?.id ?? null;
                const profile = draft.asr.api_profiles.find((item) => item.id === profileId);
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
              <small>{t("settings.translation.ownVoiceDescription")}</small>
            </span>
          </div>
          <div className="translation-own-voice-fields">
            <PreferenceToggle
              title={t("settings.translation.translateOwnVoice")}
              description={t("settings.translation.translateOwnVoiceDescription")}
              checked={draft.translation.translate_microphone}
              disabled={controlsDisabled || !draft.asr.api_profiles.length}
              onChange={(translate_microphone) => {
                const profileId = translate_microphone
                  ? draft.translation.profile_id ?? draft.asr.api_profiles[0]?.id ?? null
                  : draft.translation.profile_id;
                const profile = draft.asr.api_profiles.find((item) => item.id === profileId);
                update({
                  ...draft.translation,
                  translate_microphone,
                  profile_id: profileId,
                  model: modelForProfile(profile, draft.translation.model),
                });
              }}
            />
            <Select
              label={t("settings.translation.ownVoiceTargetLanguage")}
              helper={draft.asr.api_profiles.length
                ? t("settings.translation.ownVoiceTargetLanguageDescription")
                : t("settings.translation.noProfiles")}
              value={draft.translation.microphone_target_language}
              disabled={controlsDisabled || !draft.translation.translate_microphone}
              options={targetLanguages.map((value) => ({
                value,
                label: t(`translation.languages.${value}`),
              }))}
              onChange={(microphone_target_language) => update({
                ...draft.translation,
                microphone_target_language: microphone_target_language as TranslationSettings["microphone_target_language"],
              })}
            />
          </div>
        </div>

        <div className="translation-config-row">
          <div className="translation-config-title">
            <Languages size={17} />
            <span>
              <strong>{t("settings.translation.targetLanguage")}</strong>
              <small>{t("settings.translation.targetLanguageDescription")}</small>
            </span>
          </div>
          <div className="translation-config-fields translation-config-fields-single">
            <Select
              label={t("settings.translation.targetLanguage")}
              value={draft.translation.target_language}
              disabled={controlsDisabled}
              options={targetLanguages.map((value) => ({
                value,
                label: t(`translation.languages.${value}`),
              }))}
              onChange={(target_language) => update({
                ...draft.translation,
                target_language: target_language as TranslationSettings["target_language"],
              })}
            />
          </div>
        </div>

        <div className="translation-config-row">
          <div className="translation-config-title">
            <Cloud size={17} />
            <span>
              <strong>{t("settings.translation.profile")}</strong>
              <small>{t("settings.translation.profileDescription")}</small>
            </span>
          </div>
          <div className={`translation-config-fields ${usesLlmProfile ? "" : "translation-config-fields-single"}`}>
            <Select
              label={t("settings.translation.profile")}
              helper={draft.asr.api_profiles.length ? undefined : t("settings.translation.noProfiles")}
              value={draft.translation.profile_id ?? ""}
              disabled={controlsDisabled || !draft.asr.api_profiles.length}
              options={[
                ...(!draft.translation.profile_id
                  ? [{ value: "", label: t("settings.translation.selectProfile") }]
                  : []),
                ...draft.asr.api_profiles.map((profile) => ({ value: profile.id, label: profile.name })),
              ]}
              onChange={(profile_id) => {
                const profile = draft.asr.api_profiles.find((item) => item.id === profile_id);
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
                <small className={modelsError ? "api-model-catalog-error" : ""}>
                  {modelsLoading
                    ? t("settings.apiManagement.loadingModels")
                    : modelsError
                      || (availableModels.length
                        ? t("settings.apiManagement.modelsAvailable", { count: availableModels.length })
                        : t("settings.translation.modelHint"))}
                </small>
                {supportsThinkingToggle && (
                  <div className="translation-thinking-toggle">
                    <span>
                      <strong>{t("settings.translation.thinkingMode")}</strong>
                      <small>{t("settings.translation.thinkingModeDescription")}</small>
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
      </div>
    </div>
  );
}
