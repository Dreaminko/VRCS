import { AudioLines, Languages } from "lucide-react";
import { useTranslation } from "react-i18next";

import { recognitionProfiles } from "../../recognition-services";
import type {
  ApiProfileView,
  AsrCapabilities,
  AsrModelRecord,
  QwenModelRecord,
  QwenRuntimeStatus,
  ProviderDefinition,
} from "../../providers/types";
import type { Settings } from "../types";
import { CloudProviderSettings } from "../recognition/CloudProviderSettings";
import { LocalRecognitionSettings, LocalRuntimeStatus } from "../recognition/LocalRecognitionSettings";
import { ManagedQwenSettings } from "../recognition/ManagedQwenSettings";
import { ModelManagerPanel } from "../recognition/ModelManagerPanel";
import { VadSettings } from "../recognition/VadSettings";
import {
  LOCAL_RECOGNITION_SOURCE,
  recognitionSourceValue,
  showsLocalRecognitionSettings,
} from "../settings-derived";
import { MANAGED_QWEN_RECOGNITION_SOURCE } from "../../recognition-services";
import type { SaveState } from "../settings-types";
import { Select } from "../SettingsControls";

type RecognitionStatus = {
  capabilities: AsrCapabilities | null;
  error?: string | null;
  modelStatusLabel: string;
  computeTypes: Settings["asr"]["local"]["compute_type"][];
  selectableModels: Array<{ id: Settings["asr"]["local"]["model"]; status: string }>;
};

type RecognitionModels = {
  installed: AsrModelRecord[];
  downloading: AsrModelRecord[];
  managed: AsrModelRecord[];
  ready: boolean;
  message: string;
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
  updateLocalAsr: <K extends keyof Settings["asr"]["local"]>(key: K, value: Settings["asr"]["local"][K]) => void;
  updateManagedQwen: <K extends keyof Settings["asr"]["managed_qwen"]>(key: K, value: Settings["asr"]["managed_qwen"][K]) => void;
  updateVad: <K extends keyof Settings["vad"]>(key: K, value: Settings["vad"][K]) => void;
  loadModels: () => Promise<void>;
  setModelDirectoryText: (value: string) => void;
  updateModelDirectory: (value: string) => void;
  chooseModelDirectory: () => Promise<void>;
  downloadModel: (model: AsrModelRecord) => Promise<void>;
  removeModel: (model: AsrModelRecord) => Promise<void>;
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
  status,
  models,
  saveState,
  actions,
}: {
  locale: string;
  draft: Settings;
  apiProfiles: ApiProfileView[];
  providerDefinitions: ProviderDefinition[];
  modelStatus: string;
  status: RecognitionStatus;
  models: RecognitionModels;
  saveState: SaveState;
  actions: RecognitionActions;
}) {
  const { t } = useTranslation();
  const usesLocalAsr = showsLocalRecognitionSettings(draft.asr.backend);
  const usesManagedQwen = draft.asr.backend === "qwen_local_managed";
  const recognitionSource = recognitionSourceValue(draft.asr);
  const sourceOptions = [
    { value: LOCAL_RECOGNITION_SOURCE, label: t("settings.recognition.localSource") },
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
        <div><AudioLines size={18} /><h2>{t("settings.recognition.title")}</h2>{usesLocalAsr && <span className="status-chip">{t("settings.recognition.status", { status: modelStatus })}</span>}</div>
      </div>
      {usesLocalAsr && <LocalRuntimeStatus capabilities={status.capabilities} />}
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
            <Select
              label={t("settings.recognition.failurePolicy")}
              value={draft.asr.cloud_failure_policy}
              options={[
                { value: "reconnect", label: t("settings.recognition.reconnect") },
                { value: "local", label: t("settings.recognition.fallbackLocal") },
              ]}
              disabled={draft.asr.backend === "local_whisper"}
              onChange={(value) => actions.updateAsr("cloud_failure_policy", value as Settings["asr"]["cloud_failure_policy"])}
            />
          </div>
        </div>
        {!usesLocalAsr && !usesManagedQwen && (
          <CloudProviderSettings
            draft={draft}
            apiProfiles={apiProfiles}
            providerDefinitions={providerDefinitions}
            disabled={false}
            onUpdateAsr={actions.updateAsr}
            onSelectService={actions.updateRecognitionService}
          />
        )}
        {usesLocalAsr && (
          <LocalRecognitionSettings
            draft={draft}
            disabled={false}
            capabilities={status.capabilities}
            asrError={status.error}
            modelStatusLabel={status.modelStatusLabel}
            computeTypes={status.computeTypes}
            selectableModels={status.selectableModels}
            onUpdateAsr={actions.updateAsr}
            onUpdateLocalAsr={actions.updateLocalAsr}
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
      {(usesLocalAsr || usesManagedQwen) && (
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
          installedModels={models.installed}
          downloadingModels={models.downloading}
          managedModels={models.managed}
          modelsReady={models.ready}
          message={models.message}
          directoryText={models.directoryText}
          saveState={saveState}
          onLoad={actions.loadModels}
          onSetDirectoryText={actions.setModelDirectoryText}
          onUpdateDirectory={actions.updateModelDirectory}
          onChooseDirectory={actions.chooseModelDirectory}
          onDownload={actions.downloadModel}
          onRemove={actions.removeModel}
        />
      )}
    </div>
  );
}
