import { useTranslation } from "react-i18next";
import {
  Check,
  Clock3,
  Download,
  FolderOpen,
  HardDrive,
  Languages,
  RefreshCw,
  Trash2,
} from "lucide-react";

import { NATIVE_APP } from "../../app-environment";
import type { AsrCapabilities, AsrModelRecord, Settings } from "../../types";
import { formatBytes, MODEL_PRESENTATION } from "../settings-derived";
import type { SaveState } from "../settings-types";
import { RangeField, Select } from "../SettingsControls";

export function RecognitionSettingsSection({
  locale,
  draft,
  disabled,
  modelStatus,
  asrCapabilities,
  asrError,
  modelStatusLabel,
  computeTypes,
  selectableModels,
  installedModels,
  downloadingModels,
  managedModels,
  modelsReady,
  modelMessage,
  modelDirectoryText,
  saveState,
  onUpdateAsr,
  onUpdateVad,
  onLoadModels,
  onSetModelDirectoryText,
  onUpdateModelDirectory,
  onChooseModelDirectory,
  onDownloadModel,
  onRemoveModel,
}: {
  locale: string;
  draft: Settings;
  disabled: boolean;
  modelStatus: string;
  asrCapabilities: AsrCapabilities | null;
  asrError?: string | null;
  modelStatusLabel: string;
  computeTypes: Settings["asr"]["compute_type"][];
  selectableModels: Array<{
    id: Settings["asr"]["model"];
    status: string;
  }>;
  installedModels: AsrModelRecord[];
  downloadingModels: AsrModelRecord[];
  managedModels: AsrModelRecord[];
  modelsReady: boolean;
  modelMessage: string;
  modelDirectoryText: string;
  saveState: SaveState;
  onUpdateAsr: <K extends keyof Settings["asr"]>(key: K, value: Settings["asr"][K]) => void;
  onUpdateVad: <K extends keyof Settings["vad"]>(key: K, value: Settings["vad"][K]) => void;
  onLoadModels: () => Promise<void>;
  onSetModelDirectoryText: (value: string) => void;
  onUpdateModelDirectory: (value: string) => void;
  onChooseModelDirectory: () => Promise<void>;
  onDownloadModel: (model: AsrModelRecord) => Promise<void>;
  onRemoveModel: (model: AsrModelRecord) => Promise<void>;
}) {
  const { t } = useTranslation();
  const updateAsr = onUpdateAsr;
  const updateVad = onUpdateVad;
  const loadModels = onLoadModels;
  const setModelDirectoryText = onSetModelDirectoryText;
  const updateModelDirectory = onUpdateModelDirectory;
  const chooseModelDirectory = onChooseModelDirectory;
  const downloadModel = onDownloadModel;
  const removeModel = onRemoveModel;
  return (
        <div className="settings-section settings-section-active recognition-section" id="settings-panel-recognition" role="tabpanel" aria-labelledby="settings-tab-recognition">
          <div className="section-heading">
            <div><Languages size={18} /><h2>{t("settings.recognition.title")}</h2><span className="status-chip">{t("settings.recognition.status", { status: modelStatus })}</span></div>
            <p>{disabled ? t("settings.recognition.stopToModify") : t("settings.recognition.applyImmediately")}</p>
          </div>
          <div className={`recognition-runtime ${asrCapabilities?.cuda.available ? "available" : "unavailable"}`}>
            <span className="recognition-runtime-dot" aria-hidden="true" />
            <div>
              <strong>{t("settings.recognition.runtime")}</strong>
              <span>
                {asrCapabilities === null
                  ? t("settings.recognition.runtimeChecking")
                  : asrCapabilities.cuda.available
                    ? t("settings.recognition.cudaAvailable", { count: asrCapabilities.cuda.device_count })
                    : asrCapabilities.cuda.device_count > 0
                      ? t("settings.recognition.cudaRuntimeMissing")
                      : t("settings.recognition.cudaUnavailable")}
              </span>
            </div>
          </div>
          <div className="recognition-config">
            <div className="recognition-config-row">
              <div className="recognition-config-title">
                <Languages size={17} />
                <span><strong>{t("settings.recognition.content")}</strong><small>{t("settings.recognition.contentDescription")}</small></span>
              </div>
              <div className="recognition-config-fields">
                <Select
                  label={t("settings.recognition.model")}
                  helper={modelStatusLabel}
                  value={draft.asr.model}
                  options={selectableModels.map((model) => ({
                    value: model.id,
                    label: `${model.id} · ${
                      model.status === "not_downloaded"
                        ? t("settings.recognition.modelState.notDownloaded")
                        : model.status === "loading"
                          ? t("settings.recognition.modelState.loading")
                          : model.status === "error"
                            ? t("settings.recognition.modelState.error")
                            : t("settings.recognition.modelState.ready")
                    }`,
                  }))}
                  disabled={disabled}
                  onChange={(value) => updateAsr("model", value as Settings["asr"]["model"])}
                />
                <Select
                  label={t("settings.recognition.language")}
                  helper={t("settings.recognition.languageDescription")}
                  value={draft.asr.language}
                  options={[
                    { value: "auto", label: t("languages.auto") },
                    { value: "en", label: t("languages.english") },
                    { value: "ja", label: t("languages.japanese") },
                    { value: "zh", label: t("languages.chinese") },
                    { value: "ko", label: t("languages.korean") },
                    { value: "es", label: t("languages.spanish") },
                    { value: "fr", label: t("languages.french") },
                    { value: "de", label: t("languages.german") },
                  ]}
                  disabled={disabled}
                  onChange={(value) => updateAsr("language", value as Settings["asr"]["language"])}
                />
              </div>
            </div>
            <div className="recognition-config-row">
              <div className="recognition-config-title">
                <HardDrive size={17} />
                <span><strong>{t("settings.recognition.execution")}</strong><small>{t("settings.recognition.executionDescription")}</small></span>
              </div>
              <div className="recognition-config-fields">
                <Select
                  label={t("settings.recognition.device")}
                  helper={asrError ?? t("settings.recognition.deviceDescription")}
                  value={draft.asr.device}
                  options={[
                    { value: "auto", label: t("common.autoSelect") },
                    { value: "cpu", label: "CPU" },
                    ...(asrCapabilities?.cuda.available ? [{ value: "cuda", label: "CUDA" }] : []),
                    ...(draft.asr.device === "cuda" && !asrCapabilities?.cuda.available
                      ? [{ value: "cuda", label: `CUDA · ${t("common.unavailable")}` }]
                      : []),
                  ]}
                  disabled={disabled}
                  onChange={(value) => updateAsr("device", value as Settings["asr"]["device"])}
                />
                <Select
                  label={t("settings.recognition.computeType")}
                  helper={t("settings.recognition.computeTypeDescription")}
                  value={draft.asr.compute_type}
                  values={computeTypes}
                  disabled={disabled}
                  onChange={(value) => updateAsr("compute_type", value as Settings["asr"]["compute_type"])}
                />
              </div>
            </div>
            <div className="recognition-config-row">
              <div className="recognition-config-title">
                <Clock3 size={17} />
                <span><strong>{t("settings.recognition.segmentation")}</strong><small>{t("settings.recognition.segmentationDescription")}</small></span>
              </div>
              <div className="recognition-config-fields">
                <RangeField
                  label={t("settings.recognition.silence")}
                  helper={t("settings.recognition.silenceDescription")}
                  value={draft.vad.silence_seconds}
                  min={0.1}
                  max={2}
                  step={0.1}
                  disabled={disabled}
                  formatValue={(value) => t("units.seconds", { value: value.toFixed(1) })}
                  onCommit={(value) => updateVad("silence_seconds", value)}
                />
                <RangeField
                  label={t("settings.recognition.maxSegment")}
                  helper={t("settings.recognition.maxSegmentDescription")}
                  value={draft.vad.max_speech_seconds}
                  min={1}
                  max={30}
                  step={1}
                  disabled={disabled}
                  formatValue={(value) => t("units.seconds", { value })}
                  onCommit={(value) => updateVad("max_speech_seconds", value)}
                />
              </div>
            </div>
          </div>
          <section className="model-section recognition-models" aria-labelledby="local-models-heading">
          <div className="section-heading">
            <div>
              <HardDrive size={18} />
              <h2 id="local-models-heading">{t("settings.recognition.localModels")}</h2>
              <span>
                {downloadingModels.length
                  ? t("settings.recognition.downloadingCount", { count: downloadingModels.length })
                  : modelsReady
                    ? t("settings.recognition.installedCount", { count: installedModels.length })
                    : t("common.loading")}
              </span>
            </div>
            <button className="secondary-button" type="button" disabled={!modelsReady} onClick={() => void loadModels()}><RefreshCw size={15} />{t("common.refresh")}</button>
          </div>

          <div className="model-directory-setting">
            <label htmlFor="model-directory">
              <span>{t("settings.recognition.modelDirectory")}</span>
              <small>{t("settings.recognition.modelDirectoryDescription")}</small>
            </label>
            <div>
              <input
                id="model-directory"
                type="text"
                value={modelDirectoryText}
                disabled={disabled || downloadingModels.length > 0 || saveState === "saving"}
                spellCheck={false}
                onChange={(event) => setModelDirectoryText(event.target.value)}
                onBlur={() => updateModelDirectory(modelDirectoryText)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    event.currentTarget.blur();
                  }
                }}
              />
              <button
                className="secondary-button"
                type="button"
                disabled={!NATIVE_APP || disabled || downloadingModels.length > 0 || saveState === "saving"}
                title={NATIVE_APP ? t("settings.recognition.chooseFolder") : t("settings.recognition.browserPathHint")}
                onClick={() => void chooseModelDirectory()}
              >
                <FolderOpen size={16} />
                {t("settings.recognition.chooseFolder")}
              </button>
            </div>
          </div>

          {!modelsReady && managedModels.length === 0 ? (
            <div className="model-list-pending" role="status">
              <RefreshCw size={17} />
              <span>{t("settings.recognition.checkingLocalModels")}</span>
            </div>
          ) : (
            <div className="model-list">
              {managedModels.map((model) => {
                const presentation = MODEL_PRESENTATION[model.id];
                const downloaded = ["downloaded", "loading", "ready"].includes(model.status);
                const downloading = model.status === "downloading";
                const percentage = Math.round(model.progress * 100);
                const sizeLabel = downloading
                  ? `${formatBytes(model.downloaded_bytes, locale)} / ${formatBytes(model.total_bytes, locale)}`
                  : formatBytes(model.total_bytes, locale);
                return (
                  <article className={`model-row model-status-${model.status}`} key={model.id}>
                    <div className="model-row-body">
                      <div className="model-row-title">
                        <strong>{presentation.name}</strong>
                        {model.active && <span className="model-active-chip">{t("settings.recognition.inUse")}</span>}
                        <span className="model-size">{sizeLabel}</span>
                      </div>
                      <p>{t(presentation.descriptionKey)}</p>
                      <code>{model.repository}</code>
                      {downloading && (
                        <div className="model-progress-wrap">
                          <div
                            className="model-progress-track"
                            role="progressbar"
                            aria-label={t("settings.recognition.downloadProgress", { name: presentation.name })}
                            aria-valuemin={0}
                            aria-valuemax={100}
                            aria-valuenow={percentage}
                          >
                            <span style={{ transform: `scaleX(${Math.max(0.02, model.progress)})` }} />
                          </div>
                          <span>{percentage}%</span>
                        </div>
                      )}
                      {model.status === "error" && model.error && (
                        <p className="model-error" role="alert">{model.error}</p>
                      )}
                    </div>
                    <div className="model-row-action">
                      {downloading ? (
                        <span className="model-download-state"><RefreshCw size={15} />{t("common.downloading")}</span>
                      ) : downloaded ? (
                        model.active ? (
                          <span className="model-ready-state"><Check size={15} />{t("common.ready")}</span>
                        ) : (
                          <button className="model-delete-button" type="button" aria-label={t("settings.recognition.deleteModel", { name: presentation.name })} onClick={() => void removeModel(model)}><Trash2 size={16} /><span>{t("common.delete")}</span></button>
                        )
                      ) : (
                        <button className="model-download-button" type="button" onClick={() => void downloadModel(model)}><Download size={16} />{model.status === "error" ? t("common.retry") : t("common.download")}</button>
                      )}
                    </div>
                  </article>
                );
              })}
            </div>
          )}
          {modelMessage && <p className="model-manager-feedback" role="status">{modelMessage}</p>}
          </section>
        </div>
  );
}
