import { Check, Download, FolderOpen, HardDrive, RefreshCw, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { NATIVE_APP } from "../../app/app-environment";
import type { AsrModelRecord, QwenModelRecord } from "../../providers/types";
import { formatBytes, MODEL_PRESENTATION } from "../settings-derived";
import type { SaveState } from "../settings-types";

export function ModelManagerPanel({
  locale,
  disabled,
  installedModels,
  downloadingModels,
  managedModels,
  modelsReady,
  qwen,
  message,
  directoryText,
  saveState,
  onLoad,
  onSetDirectoryText,
  onUpdateDirectory,
  onChooseDirectory,
  onDownload,
  onRemove,
}: {
  locale: string;
  disabled: boolean;
  installedModels: AsrModelRecord[];
  downloadingModels: AsrModelRecord[];
  managedModels: AsrModelRecord[];
  modelsReady: boolean;
  qwen: {
    models: QwenModelRecord[];
    ready: boolean;
    message: string;
    selectedId: string | null;
    onLoad: () => Promise<void>;
    onDownload: (model: QwenModelRecord) => Promise<void>;
    onCancel: (model: QwenModelRecord) => Promise<void>;
    onRemove: (model: QwenModelRecord) => Promise<void>;
  };
  message: string;
  directoryText: string;
  saveState: SaveState;
  onLoad: () => Promise<void>;
  onSetDirectoryText: (value: string) => void;
  onUpdateDirectory: (value: string) => void;
  onChooseDirectory: () => Promise<void>;
  onDownload: (model: AsrModelRecord) => Promise<void>;
  onRemove: (model: AsrModelRecord) => Promise<void>;
}) {
  const { t } = useTranslation();
  const downloadingCount = downloadingModels.length + qwen.models.filter(
    (model) => model.status === "downloading" || model.status === "verifying",
  ).length;
  const installedCount = installedModels.length + qwen.models.filter((model) => model.status === "installed").length;
  const ready = modelsReady && qwen.ready;
  return (
    <section className="model-section recognition-models" aria-labelledby="local-models-heading">
      <div className="section-heading">
        <div>
          <HardDrive size={18} />
          <h3 id="local-models-heading">{t("settings.recognition.localModels")}</h3>
          <span>
            {downloadingCount
              ? t("settings.recognition.downloadingCount", { count: downloadingCount })
              : ready
                ? t("settings.recognition.installedCount", { count: installedCount })
                : t("common.loading")}
          </span>
        </div>
        <button className="secondary-button" type="button" disabled={!modelsReady && !qwen.ready} onClick={() => void Promise.all([onLoad(), qwen.onLoad()])}><RefreshCw size={15} />{t("common.refresh")}</button>
      </div>

      <div className="model-directory-setting">
        <label htmlFor="model-directory">
          <span>{t("settings.recognition.modelDirectory")}</span>
        </label>
        <div>
          <input
            id="model-directory"
            type="text"
            value={directoryText}
            disabled={disabled || downloadingCount > 0 || saveState === "saving"}
            spellCheck={false}
            onChange={(event) => onSetDirectoryText(event.target.value)}
            onBlur={() => onUpdateDirectory(directoryText)}
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
            disabled={!NATIVE_APP || disabled || downloadingCount > 0 || saveState === "saving"}
            title={NATIVE_APP ? t("settings.recognition.chooseFolder") : t("settings.recognition.browserPathHint")}
            onClick={() => void onChooseDirectory()}
          >
            <FolderOpen size={16} />
            {t("settings.recognition.chooseFolder")}
          </button>
        </div>
      </div>

      {!ready && managedModels.length === 0 && qwen.models.length === 0 ? (
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
                      <button className="model-delete-button" type="button" aria-label={t("settings.recognition.deleteModel", { name: presentation.name })} onClick={() => void onRemove(model)}><Trash2 size={16} /><span>{t("common.delete")}</span></button>
                    )
                  ) : (
                    <button className="model-download-button" type="button" onClick={() => void onDownload(model)}><Download size={16} />{model.status === "error" ? t("common.retry") : t("common.download")}</button>
                  )}
                </div>
              </article>
            );
          })}
          {!qwen.ready && qwen.models.length === 0 && <p className="model-manager-feedback" role="status">{t("settings.recognition.checkingLocalModels")}</p>}
          {qwen.models.map((model) => {
            const busy = model.status === "downloading" || model.status === "verifying";
            const selectedModel = model.id === qwen.selectedId;
            return <article className={`model-row model-status-${model.status}`} key={model.id}>
              <div className="model-row-body">
                <div className="model-row-title">
                  <strong>Qwen3-ASR 0.6B Q8_0</strong>
                  {selectedModel && <span className="model-active-chip">{t("settings.recognition.inUse")}</span>}
                  <span className="model-size">{formatBytes(model.total_bytes, locale)}</span>
                </div>
                <p>{t(`settings.recognition.qwenStatus.${model.status}`)}</p>
                <code>{model.repository}</code>
                {busy && <div className="model-progress-wrap">
                  <div className="model-progress-track" role="progressbar" aria-label={t("settings.recognition.downloadProgress", { name: model.id })} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(model.progress * 100)}>
                    <span style={{ transform: `scaleX(${Math.max(0.02, model.progress)})` }} />
                  </div>
                  <span>{Math.round(model.progress * 100)}%</span>
                </div>}
                {model.error && <p className="model-error" role="alert">{model.error}</p>}
              </div>
              <div className="model-row-action">
                {busy ? <button className="secondary-button" type="button" onClick={() => void qwen.onCancel(model)}>{t("common.cancel")}</button>
                  : model.status === "installed" ? (
                    selectedModel ? (
                      <span className="model-ready-state"><Check size={15} />{t("common.ready")}</span>
                    ) : (
                      <button className="model-delete-button" type="button" aria-label={t("settings.recognition.deleteModel", { name: model.id })} onClick={() => void qwen.onRemove(model)}><Trash2 size={16} />{t("common.delete")}</button>
                    )
                  ) : model.status === "corrupt" ? <button className="model-delete-button" type="button" onClick={() => void qwen.onRemove(model)}><Trash2 size={16} />{t("common.delete")}</button>
                    : <button className="model-download-button" type="button" onClick={() => void qwen.onDownload(model)}><Download size={16} />{t(model.status === "error" ? "common.retry" : "common.download")}</button>}
              </div>
            </article>;
          })}
        </div>
      )}
      {message && <p className="model-manager-feedback" role="status">{message}</p>}
      {qwen.message && <p className="model-manager-feedback" role="status">{qwen.message}</p>}
    </section>
  );
}
