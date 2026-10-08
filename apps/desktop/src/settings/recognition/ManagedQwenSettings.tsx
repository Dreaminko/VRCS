import { HardDrive } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { QwenModelRecord, QwenRuntimeStatus } from "../../providers/types";
import type { Settings } from "../types";
import { Select } from "../SettingsControls";
import { RecognitionLanguageSelect } from "./RecognitionLanguageSelect";

export function ManagedQwenSettings({
  draft,
  models,
  ready,
  runtime,
  disabled,
  onUpdateAsr,
  onUpdateQwen,
}: {
  draft: Settings;
  models: QwenModelRecord[];
  ready: boolean;
  runtime: QwenRuntimeStatus | null;
  disabled: boolean;
  onUpdateAsr: <K extends keyof Settings["asr"]>(key: K, value: Settings["asr"][K]) => void;
  onUpdateQwen: <K extends keyof Settings["asr"]["managed_qwen"]>(key: K, value: Settings["asr"]["managed_qwen"][K]) => void;
}) {
  const { t } = useTranslation();
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
    {runtime && !runtime.available && <p className="model-manager-feedback" role="status">{t("settings.recognition.qwenRuntimeMissing")}</p>}
    {runtime?.fallback && <p className="model-manager-feedback" role="status">{t("settings.recognition.qwenCpuFallback")}</p>}
    {runtime?.error && <p className="model-error" role="alert">{runtime.error}</p>}
  </>;
}
