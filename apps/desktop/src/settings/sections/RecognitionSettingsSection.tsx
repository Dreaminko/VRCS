import { Languages } from "lucide-react";
import { useTranslation } from "react-i18next";

import { supportsRecognition } from "../../api-profile-purpose";
import type { AsrCapabilities, AsrModelRecord, Settings } from "../../types";
import { CloudProviderSettings } from "../recognition/CloudProviderSettings";
import { LocalRecognitionSettings, LocalRuntimeStatus } from "../recognition/LocalRecognitionSettings";
import { ModelManagerPanel } from "../recognition/ModelManagerPanel";
import { VadSettings } from "../recognition/VadSettings";
import {
  LOCAL_RECOGNITION_SOURCE,
  recognitionSourceValue,
  showsLocalRecognitionSettings,
} from "../settings-derived";
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
};

type RecognitionActions = {
  updateAsr: <K extends keyof Settings["asr"]>(key: K, value: Settings["asr"][K]) => void;
  updateRecognitionSource: (source: string) => void;
  updateLocalAsr: <K extends keyof Settings["asr"]["local"]>(key: K, value: Settings["asr"]["local"][K]) => void;
  updateVad: <K extends keyof Settings["vad"]>(key: K, value: Settings["vad"][K]) => void;
  loadModels: () => Promise<void>;
  setModelDirectoryText: (value: string) => void;
  updateModelDirectory: (value: string) => void;
  chooseModelDirectory: () => Promise<void>;
  downloadModel: (model: AsrModelRecord) => Promise<void>;
  removeModel: (model: AsrModelRecord) => Promise<void>;
};

export function RecognitionSettingsSection({
  locale,
  draft,
  disabled,
  modelStatus,
  status,
  models,
  saveState,
  actions,
}: {
  locale: string;
  draft: Settings;
  disabled: boolean;
  modelStatus: string;
  status: RecognitionStatus;
  models: RecognitionModels;
  saveState: SaveState;
  actions: RecognitionActions;
}) {
  const { t } = useTranslation();
  const usesLocalAsr = showsLocalRecognitionSettings(draft.asr.backend);
  const recognitionSource = recognitionSourceValue(draft.asr);
  const sourceOptions = [
    { value: LOCAL_RECOGNITION_SOURCE, label: t("settings.recognition.localSource") },
    ...draft.asr.api_profiles
      .filter(supportsRecognition)
      .map((profile) => {
        const providerLabel = profile.provider === "alibaba_cloud" ? "Alibaba Cloud" : "OpenAI";
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
        <div><Languages size={18} /><h2>{t("settings.recognition.title")}</h2>{usesLocalAsr && <span className="status-chip">{t("settings.recognition.status", { status: modelStatus })}</span>}</div>
        <p>{disabled ? t("settings.recognition.stopToModify") : t("settings.recognition.applyImmediately")}</p>
      </div>
      {usesLocalAsr && <LocalRuntimeStatus capabilities={status.capabilities} />}
      <div className="recognition-config">
        <div className="recognition-config-row">
          <div className="recognition-config-title">
            <Languages size={17} />
            <span><strong>{t("settings.recognition.source")}</strong><small>{t("settings.recognition.sourceDescription")}</small></span>
          </div>
          <div className="recognition-config-fields">
            <Select
              label={t("settings.recognition.source")}
              helper={t("settings.recognition.manageApiHint")}
              value={recognitionSource}
              options={sourceOptions}
              disabled={disabled}
              onChange={(value) => { if (value) actions.updateRecognitionSource(value); }}
            />
            <Select
              label={t("settings.recognition.failurePolicy")}
              value={draft.asr.cloud_failure_policy}
              options={[
                { value: "reconnect", label: t("settings.recognition.reconnect") },
                { value: "local", label: t("settings.recognition.fallbackLocal") },
              ]}
              disabled={disabled || draft.asr.backend === "local_whisper"}
              onChange={(value) => actions.updateAsr("cloud_failure_policy", value as Settings["asr"]["cloud_failure_policy"])}
            />
          </div>
        </div>
        {!usesLocalAsr && <CloudProviderSettings draft={draft} disabled={disabled} onUpdateAsr={actions.updateAsr} />}
        {usesLocalAsr && (
          <LocalRecognitionSettings
            draft={draft}
            disabled={disabled}
            capabilities={status.capabilities}
            asrError={status.error}
            modelStatusLabel={status.modelStatusLabel}
            computeTypes={status.computeTypes}
            selectableModels={status.selectableModels}
            onUpdateAsr={actions.updateAsr}
            onUpdateLocalAsr={actions.updateLocalAsr}
          />
        )}
        <VadSettings vad={draft.vad} disabled={disabled} onUpdate={actions.updateVad} />
      </div>
      {usesLocalAsr && (
        <ModelManagerPanel
          locale={locale}
          disabled={disabled}
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
