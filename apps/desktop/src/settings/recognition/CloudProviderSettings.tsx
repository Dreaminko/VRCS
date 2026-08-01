import { HardDrive } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { Settings } from "../../types";
import { Select } from "../SettingsControls";
import { useAsrCredentials } from "./useAsrCredentials";

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
  const credentials = useAsrCredentials(provider);

  return (
    <div className="recognition-config-row">
      <div className="recognition-config-title">
        <HardDrive size={17} />
        <span><strong>{cloudTitle}</strong><small>{t("settings.recognition.cloudDescription")}</small></span>
      </div>
      <div className="recognition-config-fields">
        {usesAlibabaCloud && <>
          <label className="field cloud-text-field"><span>Workspace ID</span><input value={draft.asr.qwen.workspace_id} disabled={disabled} onChange={(event) => onUpdateAsr("qwen", { ...draft.asr.qwen, workspace_id: event.target.value })} /></label>
          <Select label={t("settings.recognition.region")} value={draft.asr.qwen.region} options={[{ value: "singapore", label: "Singapore" }, { value: "china_beijing", label: "China (Beijing)" }]} disabled={disabled} onChange={(value) => onUpdateAsr("qwen", { ...draft.asr.qwen, region: value as Settings["asr"]["qwen"]["region"] })} />
          {draft.asr.backend === "qwen_realtime"
            ? <label className="field cloud-text-field cloud-context-field"><span>{t("settings.recognition.context")}</span><textarea value={draft.asr.qwen.context} disabled={disabled} placeholder={t("settings.recognition.contextDescription")} onChange={(event) => onUpdateAsr("qwen", { ...draft.asr.qwen, context: event.target.value })} /></label>
            : <label className="field cloud-text-field cloud-context-field"><span>{t("settings.recognition.context")}</span><textarea maxLength={400} value={draft.asr.fun_asr.context} disabled={disabled} placeholder={t("settings.recognition.contextDescription")} onChange={(event) => onUpdateAsr("fun_asr", { ...draft.asr.fun_asr, context: event.target.value })} /><small>{draft.asr.fun_asr.context.length}/400</small></label>}
        </>}
        {provider === "openai" && <Select label={t("settings.recognition.model")} value={draft.asr.openai.model} options={[{ value: "gpt-4o-mini-transcribe", label: "GPT-4o mini Transcribe" }, { value: "gpt-4o-transcribe", label: "GPT-4o Transcribe" }]} disabled={disabled} onChange={(value) => onUpdateAsr("openai", { model: value as Settings["asr"]["openai"]["model"] })} />}
        <label className="field cloud-text-field"><span>API Key</span><input type="password" value={credentials.apiKey} disabled={disabled} placeholder={credentials.status?.configured ? t("settings.recognition.credentialConfigured") : ""} onChange={(event) => credentials.setApiKey(event.target.value)} /></label>
        <div className="settings-inline-actions">
          <button className="secondary-button" type="button" disabled={disabled || !credentials.apiKey.trim()} onClick={() => void credentials.save()}>{t("common.save")}</button>
          <button className="secondary-button" type="button" disabled={!credentials.status?.configured} onClick={() => void credentials.test()}>{t("settings.recognition.testConnection")}</button>
          <button className="secondary-button" type="button" disabled={disabled || !credentials.status?.configured || credentials.status.source === "environment"} onClick={() => void credentials.remove()}>{t("common.delete")}</button>
        </div>
        {credentials.message && <small role="status">{credentials.message}</small>}
      </div>
    </div>
  );
}
