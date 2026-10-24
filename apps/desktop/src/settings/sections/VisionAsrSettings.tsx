import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { providersApi } from "../../providers/api";
import { MANAGED_QWEN_RECOGNITION_SOURCE } from "../../providers/recognition-services";
import type { ApiProfileView, AsrSettings, ProviderDefinition } from "../../providers/types";
import { EditableDropdownField } from "../../shared/ui/DropdownField";
import { Select } from "../SettingsControls";
import { useProfileModels } from "../hooks/useProfileModels";
import { RecognitionLanguageSelect } from "../recognition/RecognitionLanguageSelect";
import type { VisionAsrSettings as VisionAsrConfig } from "../types";
import { selectVisionAsrService, selectVisionAsrSource, supportsQuestionAsr, visionAsrProfiles, visionAsrServices } from "../vision-settings";

export function VisionAsrSettings({ config, shared, profiles, definitions, disabled, onChange }: {
  config: VisionAsrConfig | null;
  shared: AsrSettings;
  profiles: ApiProfileView[];
  definitions: ProviderDefinition[];
  disabled: boolean;
  onChange: (asr: VisionAsrConfig | null) => void;
}) {
  const { t } = useTranslation();
  const compatible = visionAsrProfiles(profiles, definitions);
  const selected = compatible.find((profile) => profile.id === config?.profile_id);
  const services = visionAsrServices(selected, definitions);
  const service = services.find((item) => item.id === config?.backend);
  const managed = config?.backend === "qwen_local_managed";
  const source = config ? managed ? MANAGED_QWEN_RECOGNITION_SOURCE : config.profile_id ?? "unavailable" : "";
  const loadModels = useCallback((profileId: string) => providersApi.recognitionServiceModels(profileId, config?.backend ?? ""), [config?.backend]);
  const catalog = useProfileModels(selected, Boolean(service?.model_listing), loadModels);
  const models = catalog.models.length ? catalog.models : service?.models ?? [];
  const globalService = definitions.flatMap((provider) => provider.services).find((item) => item.id === shared.backend);
  const unsupportedGlobal = !config && shared.backend !== "qwen_local_managed" && globalService && !supportsQuestionAsr(globalService);

  return <div className="vision-settings-group">
    <div className="vr-overlay-field-grid">
      <Select label={t("settings.vision.asrSource")} value={source} disabled={disabled}
        helper={t("settings.vision.asrDescription")}
        options={[
          { value: "", label: t("settings.vision.asrFollowGlobal") },
          { value: MANAGED_QWEN_RECOGNITION_SOURCE, label: t("settings.recognition.managedQwenSource") },
          ...compatible.map((profile) => ({ value: profile.id, label: profile.name === profile.provider_display_name
            ? profile.name : `${profile.name} · ${profile.provider_display_name}` })),
          ...(config && !managed && !selected ? [{ value: source, label: t("settings.recognition.selectApiProfile") }] : []),
        ]}
        onChange={(value) => onChange(selectVisionAsrSource(config, value, shared, profiles, definitions))} />
      {config && <RecognitionLanguageSelect value={config.language} disabled={disabled}
        supportedLanguages={service?.recognition_languages}
        onChange={(language) => onChange({ ...config, language })} />}
      {config && !managed && <>
        <Select label={t("settings.recognition.cloudService")} value={config.backend} disabled={disabled || !selected}
          options={services.map((item) => ({ value: item.id, label: item.display_name }))}
          onChange={(value) => {
            const next = services.find((item) => item.id === value);
            if (next) onChange(selectVisionAsrService(config, next, shared));
          }} />
        <div className="field">
          <span>{t("settings.recognition.model")}</span>
          <EditableDropdownField label={t("settings.recognition.model")} value={config.model}
            options={models.map((model) => ({ value: model, label: model }))}
            commitOnBlur disabled={disabled || !service} optionsDisabled={catalog.loading || !models.length}
            onChange={(model) => onChange({ ...config, model: model.trim() })} />
          {catalog.loading && <small>{t("settings.apiManagement.loadingModels")}</small>}
          {catalog.error && <small className="api-model-catalog-error" role="alert">{catalog.error}</small>}
        </div>
      </>}
    </div>
    {unsupportedGlobal && <p className="vision-settings-note" role="alert">{t("settings.vision.asrUnsupportedGlobal")}</p>}
    {managed && <p className="vision-settings-note">{t("settings.vision.asrManagedHint")}</p>}
  </div>;
}
