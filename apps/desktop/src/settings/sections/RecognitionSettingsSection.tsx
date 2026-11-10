import { AudioLines, HardDrive, Languages } from "lucide-react";
import { useTranslation } from "react-i18next";

import { recognitionProfiles } from "../../providers/recognition-services";
import type {
  ApiProfileView,
  ProviderDefinition,
} from "../../providers/types";
import type { Settings } from "../types";
import { CloudProviderSettings } from "../recognition/CloudProviderSettings";
import { ManagedQwenSettings } from "../recognition/ManagedQwenSettings";
import { VadSettings } from "../recognition/VadSettings";
import { recognitionSourceValue } from "../settings-derived";
import { MANAGED_QWEN_RECOGNITION_SOURCE } from "../../providers/recognition-services";
import type { RecognitionActions, RecognitionModels } from "../settings-types";
import { Select } from "../SettingsControls";
import { CapabilityNotice, usePlatformCapabilities } from "../../core-client/capabilities";

export function RecognitionSettingsSection({
  draft,
  apiProfiles,
  providerDefinitions,
  modelStatus,
  models,
  actions,
  onManageLocalModels,
}: {
  draft: Settings;
  apiProfiles: ApiProfileView[];
  providerDefinitions: ProviderDefinition[];
  modelStatus: string;
  models: RecognitionModels;
  actions: RecognitionActions;
  onManageLocalModels?: () => void;
}) {
  const { t } = useTranslation();
  const { core } = usePlatformCapabilities();
  const usesManagedQwen = draft.asr.backend === "qwen_local_managed";
  const recognitionSource = recognitionSourceValue(draft.asr);
  const sourceOptions = [
    { value: MANAGED_QWEN_RECOGNITION_SOURCE, label: t("settings.recognition.managedQwenSource"),
      disabled: !core?.inference.qwen_cpu.supported, description: !core?.inference.qwen_cpu.supported ? t("platform.qwen_unavailable") : undefined },
    ...recognitionProfiles(apiProfiles)
      .map((profile) => {
        const providerLabel = profile.provider_display_name;
        return {
          value: profile.id,
          label: profile.name.toLocaleLowerCase() === providerLabel.toLocaleLowerCase()
            ? profile.name
            : `${profile.name} · ${providerLabel}`,
        };
      }),
  ];
  if (!recognitionSource) {
    sourceOptions.unshift({ value: "", label: t("settings.recognition.selectApiProfile") });
  }

  return (
    <div className="settings-section settings-section-active recognition-section" id="settings-panel-recognition" role="tabpanel" aria-labelledby="settings-tab-recognition">
      <div className="section-heading">
        <div><AudioLines size={18} /><h2>{t("settings.recognition.title")}</h2>{usesManagedQwen && <span className="status-chip">{t("settings.recognition.status", { status: modelStatus })}</span>}</div>
        {usesManagedQwen && onManageLocalModels && <button className="secondary-button" type="button"
          onClick={onManageLocalModels}>
          <HardDrive size={15} />{t("settings.modelAssignments.manageLocalModels")}
        </button>}
      </div>
      <div className="recognition-config">
        <div className="recognition-config-row recognition-source-row">
          <div className="recognition-config-title">
            <Languages size={17} />
            <span><strong>{t("settings.recognition.source")}</strong></span>
          </div>
          <div className="recognition-config-fields">
            <Select
              label={t("settings.recognition.source")}
              hideLabel
              value={recognitionSource}
              options={sourceOptions}
              disabled={false}
              onChange={(value) => { if (value) actions.updateRecognitionSource(value); }}
            />
          </div>
        </div>
        {!usesManagedQwen && (
          <CloudProviderSettings
            draft={draft}
            apiProfiles={apiProfiles}
            providerDefinitions={providerDefinitions}
            disabled={false}
            onUpdateAsr={actions.updateAsr}
            onSelectService={actions.updateRecognitionService}
          />
        )}
        {usesManagedQwen && <CapabilityNotice capability={core?.inference.qwen_cpu} />}
        {usesManagedQwen && <ManagedQwenSettings
          draft={draft}
          models={models.qwen}
          ready={models.qwenReady}
          runtime={models.qwenRuntime}
          disabled={!core?.inference.qwen_cpu.supported}
          onUpdateAsr={actions.updateAsr}
          onUpdateQwen={actions.updateManagedQwen}
        />}
        <VadSettings vad={draft.vad} backend={draft.asr.backend} disabled={false} onUpdate={actions.updateVad} />
      </div>
    </div>
  );
}
