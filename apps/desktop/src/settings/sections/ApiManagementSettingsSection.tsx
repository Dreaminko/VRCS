import { Cloud, KeyRound, ShieldCheck } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { Settings } from "../../types";
import { useAsrCredentials } from "../recognition/useAsrCredentials";
import type { CloudCredentialProvider } from "../recognition/useAsrCredentials";
import { Select } from "../SettingsControls";

function CredentialEditor({
  provider,
  disabled,
}: {
  provider: CloudCredentialProvider;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const credentials = useAsrCredentials(provider);
  const environmentManaged = credentials.status?.source === "environment";
  const statusText = credentials.loading
    ? t("settings.apiManagement.checking")
    : credentials.status?.configured
      ? t("settings.apiManagement.configured")
      : t("settings.apiManagement.notConfigured");
  const sourceText = environmentManaged
    ? t("settings.apiManagement.sourceEnvironment")
    : credentials.status?.source === "credential_manager"
      ? t("settings.apiManagement.sourceCredentialManager")
      : null;

  return (
    <div className="api-credential-editor">
      <div className={`api-credential-status ${credentials.status?.configured ? "configured" : ""}`}>
        <span aria-hidden="true" />
        <strong>{statusText}</strong>
        {sourceText && <small>{sourceText}</small>}
      </div>
      <label className="field cloud-text-field">
        <span>{t("settings.apiManagement.apiKey")}</span>
        <input
          type="password"
          value={credentials.apiKey}
          autoComplete="off"
          spellCheck={false}
          disabled={disabled || environmentManaged || credentials.loading}
          placeholder={credentials.status?.configured ? t("settings.apiManagement.credentialConfigured") : ""}
          onChange={(event) => credentials.setApiKey(event.target.value)}
        />
      </label>
      <div className="settings-inline-actions">
        <button className="secondary-button" type="button" disabled={disabled || environmentManaged || !credentials.apiKey.trim()} onClick={() => void credentials.save()}>{t("common.save")}</button>
        <button className="secondary-button" type="button" disabled={credentials.loading || !credentials.status?.configured} onClick={() => void credentials.test()}>{t("settings.apiManagement.testConnection")}</button>
        <button className="secondary-button" type="button" disabled={disabled || environmentManaged || !credentials.status?.configured} onClick={() => void credentials.remove()}>{t("common.delete")}</button>
      </div>
      {environmentManaged && <small className="api-environment-note">{t("settings.apiManagement.environmentManaged")}</small>}
      {credentials.message && <small className="api-credential-message" role="status">{credentials.message}</small>}
    </div>
  );
}

export function ApiManagementSettingsSection({
  draft,
  disabled,
  onUpdateAsr,
}: {
  draft: Settings;
  disabled: boolean;
  onUpdateAsr: <K extends keyof Settings["asr"]>(key: K, value: Settings["asr"][K]) => void;
}) {
  const { t } = useTranslation();

  return (
    <div className="settings-section settings-section-active api-section" id="settings-panel-api" role="tabpanel" aria-labelledby="settings-tab-api">
      <div className="section-heading">
        <div><KeyRound size={18} /><h2>{t("settings.apiManagement.title")}</h2></div>
        <p>{t("settings.apiManagement.subtitle")}</p>
      </div>
      <div className="api-security-note">
        <ShieldCheck size={18} aria-hidden="true" />
        <p>{t("settings.apiManagement.securityNotice")}</p>
      </div>
      <div className="api-provider-list">
        <section className="api-provider-row" aria-labelledby="api-provider-alibaba">
          <div className="api-provider-title">
            <Cloud size={18} aria-hidden="true" />
            <span>
              <strong id="api-provider-alibaba">Alibaba Cloud</strong>
              <small>{t("settings.apiManagement.alibabaDescription")}</small>
            </span>
          </div>
          <div className="api-provider-fields">
            <div className="api-provider-endpoint">
              <label className="field cloud-text-field">
                <span>Workspace ID</span>
                <input value={draft.asr.qwen.workspace_id} disabled={disabled} spellCheck={false} onChange={(event) => onUpdateAsr("qwen", { ...draft.asr.qwen, workspace_id: event.target.value })} />
              </label>
              <Select label={t("settings.apiManagement.region")} value={draft.asr.qwen.region} options={[{ value: "singapore", label: "Singapore" }, { value: "china_beijing", label: "China (Beijing)" }]} disabled={disabled} onChange={(value) => onUpdateAsr("qwen", { ...draft.asr.qwen, region: value as Settings["asr"]["qwen"]["region"] })} />
            </div>
            <CredentialEditor provider="qwen" disabled={disabled} />
          </div>
        </section>
        <section className="api-provider-row" aria-labelledby="api-provider-openai">
          <div className="api-provider-title">
            <Cloud size={18} aria-hidden="true" />
            <span>
              <strong id="api-provider-openai">OpenAI</strong>
              <small>{t("settings.apiManagement.openaiDescription")}</small>
            </span>
          </div>
          <div className="api-provider-fields">
            <CredentialEditor provider="openai" disabled={disabled} />
          </div>
        </section>
      </div>
    </div>
  );
}
