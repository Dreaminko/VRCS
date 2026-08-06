import { HardDrive } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { Settings } from "../../types";
import { Select } from "../SettingsControls";

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
  const cloudTitle = draft.asr.backend === "fun_asr_realtime" ? "Fun-ASR" : provider === "qwen" ? "Qwen3 ASR" : "OpenAI";

  return (
    <div className="recognition-config-row">
      <div className="recognition-config-title">
        <HardDrive size={17} />
        <span><strong>{cloudTitle}</strong><small>{t("settings.recognition.cloudDescription")}</small></span>
      </div>
      <div className="recognition-config-fields">
        {usesAlibabaCloud && <>
          {draft.asr.backend === "qwen_realtime"
            ? <label className="field cloud-text-field cloud-context-field"><span>{t("settings.recognition.context")}</span><textarea value={draft.asr.qwen.context} disabled={disabled} placeholder={t("settings.recognition.contextDescription")} onChange={(event) => onUpdateAsr("qwen", { ...draft.asr.qwen, context: event.target.value })} /></label>
            : <label className="field cloud-text-field cloud-context-field"><span>{t("settings.recognition.context")}</span><textarea maxLength={400} value={draft.asr.fun_asr.context} disabled={disabled} placeholder={t("settings.recognition.contextDescription")} onChange={(event) => onUpdateAsr("fun_asr", { ...draft.asr.fun_asr, context: event.target.value })} /><small>{draft.asr.fun_asr.context.length}/400</small></label>}
        </>}
        {provider === "openai" && <Select label={t("settings.recognition.model")} value={draft.asr.openai.model} options={[{ value: "gpt-4o-mini-transcribe", label: "GPT-4o mini Transcribe" }, { value: "gpt-4o-transcribe", label: "GPT-4o Transcribe" }]} disabled={disabled} onChange={(value) => onUpdateAsr("openai", { model: value as Settings["asr"]["openai"]["model"] })} />}
        <small className="cloud-api-hint">{t("settings.recognition.apiManagedCentrally")}</small>
      </div>
    </div>
  );
}
