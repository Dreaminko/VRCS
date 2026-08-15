import { HardDrive } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { Settings } from "../../types";
import { Select } from "../SettingsControls";
import { RecognitionLanguageSelect } from "./RecognitionLanguageSelect";

export function CloudProviderSettings({
  draft,
  disabled,
  onUpdateAsr,
}: {
  draft: Settings;
  disabled: boolean;
  onUpdateAsr: <K extends keyof Settings["asr"]>(key: K, value: Settings["asr"][K]) => void;
}) {
  const { t } = useTranslation();
  const usesAlibabaCloud = draft.asr.backend === "qwen_realtime" || draft.asr.backend === "fun_asr_realtime";
  const provider = usesAlibabaCloud ? "qwen" : "openai";
  const providerId = usesAlibabaCloud ? "alibaba_cloud" : "openai";
  const selectedProfile = draft.asr.api_profiles.find(
    (profile) => profile.id === draft.asr.active_api_profiles[providerId],
  );
  const cloudTitle = selectedProfile?.name ?? (usesAlibabaCloud ? "Alibaba Cloud" : "OpenAI");

  return (
    <div className="recognition-config-row">
      <div className="recognition-config-title">
        <HardDrive size={17} />
        <span><strong>{cloudTitle}</strong></span>
      </div>
      <div className="recognition-config-fields">
        {usesAlibabaCloud && <>
          <Select
            label={t("settings.recognition.cloudService")}
            value={draft.asr.backend}
            options={[
              { value: "qwen_realtime", label: "Qwen3 ASR · Streaming" },
              { value: "fun_asr_realtime", label: "Fun-ASR · Streaming" },
            ]}
            disabled={disabled}
            onChange={(value) => onUpdateAsr("backend", value as Settings["asr"]["backend"])}
          />
          {draft.asr.backend === "qwen_realtime"
            ? <label className="field cloud-text-field cloud-context-field"><span>{t("settings.recognition.context")}</span><textarea value={draft.asr.qwen.context} disabled={disabled} placeholder={t("settings.recognition.contextDescription")} onChange={(event) => onUpdateAsr("qwen", { ...draft.asr.qwen, context: event.target.value })} /></label>
            : <label className="field cloud-text-field cloud-context-field"><span>{t("settings.recognition.context")}</span><textarea maxLength={400} value={draft.asr.fun_asr.context} disabled={disabled} placeholder={t("settings.recognition.contextDescription")} onChange={(event) => onUpdateAsr("fun_asr", { ...draft.asr.fun_asr, context: event.target.value })} /><small>{draft.asr.fun_asr.context.length}/400</small></label>}
        </>}
        {provider === "openai" && <Select label={t("settings.recognition.model")} value={draft.asr.openai.model} options={[{ value: "gpt-4o-mini-transcribe", label: "GPT-4o mini Transcribe" }, { value: "gpt-4o-transcribe", label: "GPT-4o Transcribe" }]} disabled={disabled} onChange={(value) => onUpdateAsr("openai", { model: value as Settings["asr"]["openai"]["model"] })} />}
        <RecognitionLanguageSelect
          value={draft.asr.language}
          disabled={disabled}
          onChange={(value) => onUpdateAsr("language", value)}
        />
        <small className="cloud-api-hint">{t("settings.recognition.selectedApiHint", { name: cloudTitle })}</small>
      </div>
    </div>
  );
}
