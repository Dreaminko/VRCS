import { useState } from "react";
import { Download, HardDrive } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { QwenModelRecord, QwenRuntimeStatus } from "../../providers/types";
import type { Settings } from "../types";
import { Select } from "../SettingsControls";
import { formatBytes } from "../settings-derived";
import { RecognitionLanguageSelect } from "./RecognitionLanguageSelect";

export function ManagedQwenSettings({
  draft,
  models,
  ready,
  runtime,
  disabled,
  onUpdateAsr,
  onUpdateQwen,
  onDownloadRuntime,
  onCancelRuntimeDownload,
}: {
  draft: Settings;
  models: QwenModelRecord[];
  ready: boolean;
  runtime: QwenRuntimeStatus | null;
  disabled: boolean;
  onUpdateAsr: <K extends keyof Settings["asr"]>(key: K, value: Settings["asr"][K]) => void;
  onUpdateQwen: <K extends keyof Settings["asr"]["managed_qwen"]>(key: K, value: Settings["asr"]["managed_qwen"][K]) => void;
  onDownloadRuntime: () => Promise<void>;
  onCancelRuntimeDownload: () => Promise<void>;
}) {
  const { t, i18n } = useTranslation();
  const [runtimeActionBusy, setRuntimeActionBusy] = useState(false);
  const installation = runtime?.installation;
  const installing = installation?.status === "downloading" || installation?.status === "verifying";
  const runRuntimeAction = async (action: () => Promise<void>) => {
    setRuntimeActionBusy(true);
    try {
      await action();
    } finally {
      setRuntimeActionBusy(false);
    }
  };
  return <>
    {runtime && !runtime.available && installation && <article className={`model-row model-status-${installation.status}`}>
      <div className="model-row-body">
        <div className="model-row-title">
          <strong>{t("settings.recognition.qwenRuntimeComponents")}</strong>
          <span className="model-size">{formatBytes(installation.total_bytes, i18n.resolvedLanguage ?? i18n.language)}</span>
        </div>
        <p role="status">{installing
          ? t(`settings.recognition.qwenStatus.${installation.status}`)
          : t("settings.recognition.qwenRuntimeMissing")}</p>
        {installing && <div className="model-progress-wrap">
          <div className="model-progress-track" role="progressbar" aria-label={t("settings.recognition.downloadProgress", { name: t("settings.recognition.qwenRuntimeComponents") })} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(installation.progress * 100)}>
            <span style={{ transform: `scaleX(${Math.max(0.02, installation.progress)})` }} />
          </div>
          <span>{Math.round(installation.progress * 100)}%</span>
        </div>}
        {installation.error && <p className="model-error" role="alert">{installation.error}</p>}
      </div>
      <div className="model-row-action">
        {installing
          ? <button className="secondary-button" type="button" disabled={disabled || runtimeActionBusy} onClick={() => void runRuntimeAction(onCancelRuntimeDownload)}>{t("common.cancel")}</button>
          : <button className="model-download-button" type="button" disabled={disabled || runtimeActionBusy} onClick={() => void runRuntimeAction(onDownloadRuntime)}><Download size={16} />{t(installation.status === "error" ? "common.retry" : "common.download")}</button>}
      </div>
    </article>}
    <div className="recognition-config-row">
      <div className="recognition-config-title">
        <HardDrive size={17} />
        <span><strong>{t("settings.recognition.content")}</strong></span>
      </div>
      <div className="recognition-config-fields">
        <Select
          label={t("settings.recognition.model")}
          value={draft.asr.managed_qwen.package_id}
          options={[
            ...(!draft.asr.managed_qwen.package_id ? [{ value: "", label: t("settings.recognition.modelStatus.notSelected") }] : []),
            ...(models.length
            ? models.map((model) => ({ value: model.id, label: `Qwen3-ASR 0.6B Q8_0 · ${t(`settings.recognition.qwenStatus.${model.status}`)}` }))
            : draft.asr.managed_qwen.package_id ? [{ value: draft.asr.managed_qwen.package_id, label: "Qwen3-ASR 0.6B Q8_0" }] : []),
          ]}
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
            ...(runtime?.gpu_devices.length ? [{ value: "gpu", label: "GPU (Vulkan)" }] : []),
            ...(draft.asr.managed_qwen.device === "gpu" && !runtime?.gpu_devices.length
              ? [{ value: "gpu", label: `GPU (Vulkan) · ${t("common.unavailable")}` }]
              : []),
          ]}
          disabled={disabled}
          onChange={(value) => onUpdateQwen("device", value as "auto" | "cpu" | "gpu")}
        />
      </div>
    </div>
    {runtime?.fallback && <p className="model-manager-feedback" role="status">{t("settings.recognition.qwenCpuFallback")}</p>}
    {runtime?.error && <p className="model-error" role="alert">{runtime.error}</p>}
  </>;
}
