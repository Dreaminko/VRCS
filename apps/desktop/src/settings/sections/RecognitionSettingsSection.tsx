import { AudioLines, Languages } from "lucide-react";
import { useTranslation } from "react-i18next";

import { recognitionProfiles } from "../../recognition-services";
import type {
  ApiProfileView,
  QwenModelRecord,
  QwenRuntimeStatus,
  ProviderDefinition,
} from "../../providers/types";
import type { Settings } from "../types";
import { CloudProviderSettings } from "../recognition/CloudProviderSettings";
import { ManagedQwenSettings } from "../recognition/ManagedQwenSettings";
import { ModelManagerPanel } from "../recognition/ModelManagerPanel";
import { VadSettings } from "../recognition/VadSettings";
import { recognitionSourceValue } from "../settings-derived";
import { MANAGED_QWEN_RECOGNITION_SOURCE } from "../../recognition-services";
import type { SaveState } from "../settings-types";
import { Select } from "../SettingsControls";

type RecognitionModels = {
  directoryText: string;
  qwen: QwenModelRecord[];
  qwenReady: boolean;
  qwenRuntime: QwenRuntimeStatus | null;
  qwenMessage: string;
};

type RecognitionActions = {
  updateAsr: <K extends keyof Settings["asr"]>(key: K, value: Settings["asr"][K]) => void;
  updateRecognitionSource: (source: string) => void;
  updateRecognitionService: (serviceId: string) => void;
  updateManagedQwen: <K extends keyof Settings["asr"]["managed_qwen"]>(key: K, value: Settings["asr"]["managed_qwen"][K]) => void;
  updateVad: <K extends keyof Settings["vad"]>(key: K, value: Settings["vad"][K]) => void;
  setModelDirectoryText: (value: string) => void;
  updateModelDirectory: (value: string) => void;
  chooseModelDirectory: () => Promise<void>;
  loadQwenModels: () => Promise<void>;
  downloadQwenModel: (model: QwenModelRecord) => Promise<void>;
  downloadQwenRuntime: () => Promise<void>;
  cancelQwenRuntimeDownload: () => Promise<void>;
  cancelQwenDownload: (model: QwenModelRecord) => Promise<void>;
  verifyQwenModel: (model: QwenModelRecord) => Promise<void>;
  removeQwenModel: (model: QwenModelRecord) => Promise<void>;
};

export function RecognitionSettingsSection({
  locale,
  draft,
  apiProfiles,
  providerDefinitions,
  modelStatus,
  models,
  saveState,
  actions,
}: {
  locale: string;
  draft: Settings;
  apiProfiles: ApiProfileView[];
  providerDefinitions: ProviderDefinition[];
  modelStatus: string;
  models: RecognitionModels;
  saveState: SaveState;
  actions: RecognitionActions;
}) {
  const { t } = useTranslation();
  const usesManagedQwen = draft.asr.backend === "qwen_local_managed";
  const recognitionSource = recognitionSourceValue(draft.asr);
  const sourceOptions = [
    { value: MANAGED_QWEN_RECOGNITION_SOURCE, label: t("settings.recognition.managedQwenSource") },
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
      </div>
      <div className="recognition-config">
        <div className="recognition-config-row">
          <div className="recognition-config-title">
            <Languages size={17} />
            <span><strong>{t("settings.recognition.source")}</strong></span>
          </div>
          <div className="recognition-config-fields">
            <Select
              label={t("settings.recognition.source")}
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
        {usesManagedQwen && <ManagedQwenSettings
          draft={draft}
          models={models.qwen}
          ready={models.qwenReady}
          runtime={models.qwenRuntime}
          disabled={false}
          onUpdateAsr={actions.updateAsr}
          onUpdateQwen={actions.updateManagedQwen}
          onDownloadRuntime={actions.downloadQwenRuntime}
          onCancelRuntimeDownload={actions.cancelQwenRuntimeDownload}
        />}
        <VadSettings vad={draft.vad} disabled={false} onUpdate={actions.updateVad} />
      </div>
      {usesManagedQwen && (
        <ModelManagerPanel
          qwen={{
            models: models.qwen,
            ready: models.qwenReady,
            message: models.qwenMessage,
            selectedId: draft.asr.backend === "qwen_local_managed" ? draft.asr.managed_qwen.package_id : null,
            onLoad: actions.loadQwenModels,
            onDownload: actions.downloadQwenModel,
            onCancel: actions.cancelQwenDownload,
            onRemove: actions.removeQwenModel,
          }}
          locale={locale}
          disabled={false}
          directoryText={models.directoryText}
          saveState={saveState}
          onSetDirectoryText={actions.setModelDirectoryText}
          onUpdateDirectory={actions.updateModelDirectory}
          onChooseDirectory={actions.chooseModelDirectory}
        />
      )}
    </div>
  );
}
