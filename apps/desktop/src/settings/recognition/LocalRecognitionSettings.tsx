import { HardDrive, Languages } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { AsrCapabilities, Settings } from "../../types";
import { Select } from "../SettingsControls";

export function LocalRuntimeStatus({ capabilities }: { capabilities: AsrCapabilities | null }) {
  const { t } = useTranslation();
  return (
    <div className={`recognition-runtime ${capabilities?.cuda.available ? "available" : "unavailable"}`}>
      <span className="recognition-runtime-dot" aria-hidden="true" />
      <div>
        <strong>{t("settings.recognition.runtime")}</strong>
        <span>
          {capabilities === null
            ? t("settings.recognition.runtimeChecking")
            : capabilities.cuda.available
              ? t("settings.recognition.cudaAvailable", { count: capabilities.cuda.device_count })
              : capabilities.cuda.device_count > 0
                ? t("settings.recognition.cudaRuntimeMissing")
                : t("settings.recognition.cudaUnavailable")}
        </span>
      </div>
    </div>
  );
}

export function LocalRecognitionSettings({
  draft,
  disabled,
  capabilities,
  asrError,
  modelStatusLabel,
  computeTypes,
  selectableModels,
  onUpdateAsr,
  onUpdateLocalAsr,
}: {
  draft: Settings;
  disabled: boolean;
  capabilities: AsrCapabilities | null;
  asrError?: string | null;
  modelStatusLabel: string;
  computeTypes: Settings["asr"]["local"]["compute_type"][];
  selectableModels: Array<{ id: Settings["asr"]["local"]["model"]; status: string }>;
  onUpdateAsr: <K extends keyof Settings["asr"]>(key: K, value: Settings["asr"][K]) => void;
  onUpdateLocalAsr: <K extends keyof Settings["asr"]["local"]>(key: K, value: Settings["asr"]["local"][K]) => void;
}) {
  const { t } = useTranslation();
  return <>
    <div className="recognition-config-row">
      <div className="recognition-config-title">
        <Languages size={17} />
        <span><strong>{t("settings.recognition.content")}</strong><small>{t("settings.recognition.contentDescription")}</small></span>
      </div>
      <div className="recognition-config-fields">
        <Select
          label={t("settings.recognition.model")}
          helper={modelStatusLabel}
          value={draft.asr.local.model}
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
          onChange={(value) => onUpdateLocalAsr("model", value as Settings["asr"]["local"]["model"])}
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
          onChange={(value) => onUpdateAsr("language", value as Settings["asr"]["language"])}
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
          value={draft.asr.local.device}
          options={[
            { value: "auto", label: t("common.autoSelect") },
            { value: "cpu", label: "CPU" },
            ...(capabilities?.cuda.available ? [{ value: "cuda", label: "CUDA" }] : []),
            ...(draft.asr.local.device === "cuda" && !capabilities?.cuda.available
              ? [{ value: "cuda", label: `CUDA · ${t("common.unavailable")}` }]
              : []),
          ]}
          disabled={disabled}
          onChange={(value) => onUpdateLocalAsr("device", value as Settings["asr"]["local"]["device"])}
        />
        <Select
          label={t("settings.recognition.computeType")}
          helper={t("settings.recognition.computeTypeDescription")}
          value={draft.asr.local.compute_type}
          values={computeTypes}
          disabled={disabled}
          onChange={(value) => onUpdateLocalAsr("compute_type", value as Settings["asr"]["local"]["compute_type"])}
        />
      </div>
    </div>
  </>;
}
