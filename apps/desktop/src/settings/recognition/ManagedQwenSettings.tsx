import { Download, HardDrive, RefreshCw, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { QwenModelRecord, QwenRuntimeStatus } from "../../providers/types";
import type { Settings } from "../types";
import { formatBytes } from "../settings-derived";
import { Select } from "../SettingsControls";
import { RecognitionLanguageSelect } from "./RecognitionLanguageSelect";

export function ManagedQwenSettings({
  locale,
  draft,
  models,
  ready,
  runtime,
  message,
  disabled,
  onUpdateAsr,
  onUpdateQwen,
  onRefresh,
  onDownload,
  onCancel,
  onVerify,
  onRemove,
}: {
  locale: string;
  draft: Settings;
  models: QwenModelRecord[];
  ready: boolean;
  runtime: QwenRuntimeStatus | null;
  message: string;
  disabled: boolean;
  onUpdateAsr: <K extends keyof Settings["asr"]>(key: K, value: Settings["asr"][K]) => void;
  onUpdateQwen: <K extends keyof Settings["asr"]["managed_qwen"]>(key: K, value: Settings["asr"]["managed_qwen"][K]) => void;
  onRefresh: () => Promise<void>;
  onDownload: (model: QwenModelRecord) => Promise<void>;
  onCancel: (model: QwenModelRecord) => Promise<void>;
  onVerify: (model: QwenModelRecord) => Promise<void>;
  onRemove: (model: QwenModelRecord) => Promise<void>;
}) {
  const { t } = useTranslation();
  const selected = models.find((model) => model.id === draft.asr.managed_qwen.package_id);
  return <>
    <div className="recognition-config-row">
      <div className="recognition-config-title">
        <HardDrive size={17} />
        <span><strong>{t("settings.recognition.content")}</strong></span>
      </div>
      <div className="recognition-config-fields">
        <Select
          label={t("settings.recognition.model")}
          value={draft.asr.managed_qwen.package_id}
          options={models.length
            ? models.map((model) => ({ value: model.id, label: `Qwen3-ASR 0.6B Q8_0 · ${t(`settings.recognition.qwenStatus.${model.status}`)}` }))
            : [{ value: draft.asr.managed_qwen.package_id, label: "Qwen3-ASR 0.6B Q8_0" }]}
          disabled={disabled || !ready}
          onChange={(value) => onUpdateQwen("package_id", value)}
        />
        <RecognitionLanguageSelect
          value={draft.asr.language}
          disabled={disabled}
          onChange={(value) => onUpdateAsr("language", value)}
        />
      </div>
    </div>
    <div className="recognition-config-row">
      <div className="recognition-config-title">
        <HardDrive size={17} />
        <span><strong>{t("settings.recognition.execution")}</strong></span>
      </div>
      <div className="recognition-config-fields">
        <Select
          label={t("settings.recognition.device")}
          value={draft.asr.managed_qwen.device}
          options={[
            { value: "auto", label: t("common.autoSelect") },
            { value: "cpu", label: "CPU" },
          ]}
          disabled={disabled}
          onChange={(value) => onUpdateQwen("device", value as "auto" | "cpu")}
        />
      </div>
    </div>
    <section className="model-section recognition-models" aria-labelledby="qwen-models-heading">
      <div className="section-heading">
        <div><HardDrive size={18} /><h3 id="qwen-models-heading">{t("settings.recognition.qwenModels")}</h3></div>
        <button className="secondary-button" type="button" disabled={!ready} onClick={() => void onRefresh()}><RefreshCw size={15} />{t("common.refresh")}</button>
      </div>
      {runtime && !runtime.available && <p className="model-manager-feedback" role="status">{t("settings.recognition.qwenRuntimeMissing")}</p>}
      {!ready && models.length === 0 && <div className="model-list-pending" role="status">{t("settings.recognition.checkingLocalModels")}</div>}
      <div className="model-list">
        {models.map((model) => {
          const busy = model.status === "downloading" || model.status === "verifying";
          const selectedModel = draft.asr.backend === "qwen_local_managed" && model.id === draft.asr.managed_qwen.package_id;
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
              {busy ? <button className="secondary-button" type="button" onClick={() => void onCancel(model)}>{t("common.cancel")}</button>
                : model.status === "installed" ? <>
                  <button className="secondary-button" type="button" onClick={() => void onVerify(model)}>{t("settings.recognition.verifyModel")}</button>
                  {!selectedModel && <button className="model-delete-button" type="button" aria-label={t("settings.recognition.deleteModel", { name: model.id })} onClick={() => void onRemove(model)}><Trash2 size={16} />{t("common.delete")}</button>}
                </> : model.status === "corrupt" ? <button className="model-delete-button" type="button" onClick={() => void onRemove(model)}><Trash2 size={16} />{t("common.delete")}</button>
                  : <button className="model-download-button" type="button" onClick={() => void onDownload(model)}><Download size={16} />{t(model.status === "error" ? "common.retry" : "common.download")}</button>}
            </div>
          </article>;
        })}
      </div>
      {selected && selected.status !== "installed" && <p className="model-manager-feedback" role="status">{t("settings.recognition.qwenInstallRequired")}</p>}
      {message && <p className="model-manager-feedback" role="status">{message}</p>}
    </section>
  </>;
}
