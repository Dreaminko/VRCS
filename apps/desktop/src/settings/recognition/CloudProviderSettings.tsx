import { useEffect, useState } from "react";
import { HardDrive, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";

import { providersApi } from "../../providers/api";
import { localizedError } from "../../app/app-utils";
import {
  currentRecognitionProfile,
  currentRecognitionService,
  recognitionServicesForProfile,
  updateRecognitionServiceSettings,
} from "../../recognition-services";
import { EditableDropdownField } from "../../shared/ui/DropdownField";
import type {
  ApiProfileView,
  ProviderDefinition,
} from "../../providers/types";
import type { Settings } from "../types";
import { Select } from "../SettingsControls";
import { RecognitionLanguageSelect } from "./RecognitionLanguageSelect";

function AsrContextField({
  value,
  disabled,
  onCommit,
}: {
  value: string;
  disabled: boolean;
  onCommit: (value: string) => void;
}) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState(value);

  useEffect(() => setDraft(value), [value]);

  return (
    <label className="field cloud-text-field cloud-context-field">
      <span>{t("settings.recognition.context")}</span>
      <textarea
        value={draft}
        disabled={disabled}
        placeholder={t("settings.recognition.contextDescription")}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={() => {
          if (draft !== value) onCommit(draft);
        }}
      />
    </label>
  );
}

export function CloudProviderSettings({
  draft,
  apiProfiles,
  providerDefinitions,
  disabled,
  onUpdateAsr,
  onSelectService,
}: {
  draft: Settings;
  apiProfiles: ApiProfileView[];
  providerDefinitions: ProviderDefinition[];
  disabled: boolean;
  onUpdateAsr: <K extends keyof Settings["asr"]>(key: K, value: Settings["asr"][K]) => void;
  onSelectService: (serviceId: string) => void;
}) {
  const { t } = useTranslation();
  const selectedProfile = currentRecognitionProfile(draft.asr, apiProfiles);
  const services = recognitionServicesForProfile(selectedProfile, providerDefinitions);
  const service = currentRecognitionService(draft.asr, selectedProfile, providerDefinitions)
    ?? services[0];
  const settings = service
    ? draft.asr.service_settings[service.id] ?? { model: service.models[0] ?? "", context: "" }
    : { model: "", context: "" };
  const [discoveredModels, setDiscoveredModels] = useState<string[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelsError, setModelsError] = useState("");

  useEffect(() => {
    if (!selectedProfile || !service?.model_listing) {
      setDiscoveredModels([]);
      setModelsLoading(false);
      setModelsError("");
      return;
    }
    let cancelled = false;
    setDiscoveredModels([]);
    setModelsLoading(true);
    setModelsError("");
    void providersApi.recognitionServiceModels(selectedProfile.id, service.id).then(
      (response) => {
        if (!cancelled) setDiscoveredModels(response.models);
      },
      (reason) => {
        if (!cancelled) setModelsError(localizedError(reason, t, "errors.apiProfiles.models"));
      },
    ).finally(() => {
      if (!cancelled) setModelsLoading(false);
    });
    return () => { cancelled = true; };
  }, [selectedProfile?.id, service?.id, service?.model_listing, t]);

  const models = discoveredModels.length ? discoveredModels : service?.models ?? [];
  const updateServiceSettings = (update: Partial<{ model: string; context: string }>) => {
    if (!service) return;
    const next = updateRecognitionServiceSettings(draft.asr, service.id, update);
    onUpdateAsr("service_settings", next.service_settings);
  };

  return (
    <div className="recognition-config-row">
      <div className="recognition-config-title">
        <HardDrive size={17} />
        <span><strong>{selectedProfile?.name ?? t("settings.recognition.selectApiProfile")}</strong></span>
      </div>
      <div className="recognition-config-fields">
        {services.length > 0 && (
          <Select
            label={t("settings.recognition.cloudService")}
            value={service?.id ?? ""}
            options={services.map((item) => ({
              value: item.id,
              label: item.display_name,
              description: item.recognition_transport
                ? t(`settings.recognition.transports.${item.recognition_transport}`)
                : undefined,
            }))}
            disabled={disabled}
            onChange={onSelectService}
          />
        )}
        {service && (models.length > 0 || service.model_listing) && (
          <label className="field">
            <span>{t("settings.recognition.model")}</span>
            <EditableDropdownField
              label={t("settings.recognition.model")}
              value={settings.model}
              options={models.map((model) => ({ value: model, label: model }))}
              disabled={disabled}
              optionsDisabled={modelsLoading || !models.length}
              onChange={(model) => updateServiceSettings({ model })}
            />
          </label>
        )}
        {service?.model_listing && (
          <small className={modelsError ? "api-model-catalog-error" : "cloud-api-hint"}>
            {modelsLoading && <RefreshCw className="spin" size={12} />} {modelsLoading
              ? t("settings.apiManagement.loadingModels")
              : modelsError || t("settings.apiManagement.modelsAvailable", { count: models.length })}
          </small>
        )}
        {service?.supports_context && (
          <AsrContextField
            value={settings.context}
            disabled={disabled}
            onCommit={(context) => updateServiceSettings({ context })}
          />
        )}
        <RecognitionLanguageSelect
          value={draft.asr.language}
          disabled={disabled}
          onChange={(value) => onUpdateAsr("language", value)}
        />
        {service?.recognition_transport === "segmented_upload" && (
          <div className="cloud-transport-hint" role="note">
            <strong>{t("settings.recognition.segmentedUpload.title")}</strong>
            <small>{t("settings.recognition.segmentedUpload.description")}</small>
            {!service.partial_results && <small>{t("settings.recognition.segmentedUpload.noPartial")}</small>}
          </div>
        )}
        {selectedProfile && <small className="cloud-api-hint">{t("settings.recognition.selectedApiHint", { name: selectedProfile.name })}</small>}
      </div>
    </div>
  );
}
