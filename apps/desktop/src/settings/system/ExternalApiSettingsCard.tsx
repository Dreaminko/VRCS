import { Copy, Info, KeyRound, RadioTower, RefreshCw, Save, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { coreApi } from "../../api";
import type { CredentialStatus, ExternalApiRuntimeStatus, Settings } from "../../types";
import type { SaveState } from "../settings-types";
import { PreferenceToggle } from "../SettingsControls";

export function ExternalApiSettingsCard({ config, saveState, onChange }: {
  config: Settings["external_api"];
  saveState: SaveState;
  onChange: (patch: Partial<Settings["external_api"]>) => void;
}) {
  const { t } = useTranslation();
  const [tokenStatus, setTokenStatus] = useState<CredentialStatus | null>(null);
  const [runtimeStatus, setRuntimeStatus] = useState<ExternalApiRuntimeStatus | null>(null);
  const [tokenStatusError, setTokenStatusError] = useState(false);
  const [runtimeStatusError, setRuntimeStatusError] = useState(false);
  const [tokenBusy, setTokenBusy] = useState(false);
  const [tokenMessage, setTokenMessage] = useState("");
  const [generatedToken, setGeneratedToken] = useState("");
  const [tokenInput, setTokenInput] = useState("");

  useEffect(() => {
    let cancelled = false;
    void coreApi.externalApiTokenStatus().then(
      (status) => {
        if (!cancelled) {
          setTokenStatus(status);
          setTokenStatusError(false);
        }
      },
      () => {
        if (!cancelled) setTokenStatusError(true);
      },
    );
    void coreApi.externalApiRuntimeStatus().then(
      (status) => {
        if (!cancelled) {
          setRuntimeStatus(status);
          setRuntimeStatusError(false);
        }
      },
      () => {
        if (!cancelled) setRuntimeStatusError(true);
      },
    );
    return () => { cancelled = true; };
  }, [t]);

  const storeToken = async (token: string, reveal: boolean) => {
    if (!token.trim()) return;
    setTokenBusy(true);
    setTokenMessage("");
    try {
      setTokenStatus(await coreApi.saveExternalApiToken(token));
      setGeneratedToken(reveal ? token : "");
      setTokenInput("");
      setTokenMessage(t("settings.system.externalApi.tokenSaved"));
    } catch {
      setTokenMessage(t("settings.system.externalApi.tokenSaveFailed"));
    } finally {
      setTokenBusy(false);
    }
  };
  const generateToken = () => {
    const token = Array.from({ length: 2 }, () => crypto.randomUUID().replaceAll("-", "")).join("");
    void storeToken(token, true);
  };
  const deleteToken = async () => {
    setTokenBusy(true);
    setTokenMessage("");
    try {
      setTokenStatus(await coreApi.deleteExternalApiToken());
      setGeneratedToken("");
      setTokenMessage(t("settings.system.externalApi.tokenDeleted"));
    } catch {
      setTokenMessage(t("settings.system.externalApi.tokenDeleteFailed"));
    } finally {
      setTokenBusy(false);
    }
  };

  const runtimeState = runtimeStatusError ? "failed" : runtimeStatus?.state ?? "checking";
  const runtimeMessage = runtimeStatusError
    ? t("settings.system.externalApi.statusLoadFailed")
    : t(`settings.system.externalApi.runtime.${runtimeState}`, {
      address: runtimeStatus?.address ?? "",
      error: runtimeStatus?.error ?? "",
    });
  const tokenState = tokenStatusError
    ? "failed"
    : tokenStatus === null
      ? "checking"
      : tokenStatus.environment_override
        ? "environment"
        : tokenStatus.configured
          ? "configured"
          : "missing";
  const tokenStatusMessage = tokenStatusError
    ? t("settings.system.externalApi.tokenLoadFailed")
    : tokenStatus === null
      ? t("settings.system.externalApi.tokenChecking")
      : tokenStatus.environment_override
        ? t("settings.system.externalApi.tokenEnvironment")
        : tokenStatus.configured
          ? t("settings.system.externalApi.tokenConfigured")
          : t("settings.system.externalApi.tokenMissing");

  return (
    <div className="external-api-settings">
      <div className="external-api-heading">
        <span className="external-api-heading-icon" aria-hidden="true"><RadioTower size={18} /></span>
        <span className="external-api-heading-copy">
          <strong>{t("settings.system.externalApi.title")}</strong>
          <small>{t("settings.system.externalApi.description")}</small>
        </span>
      </div>
      <p className={`external-api-runtime-status ${runtimeState}`} role="status" aria-live="polite">
        <span className="external-api-status-dot" aria-hidden="true" />
        <span>{runtimeMessage}</span>
      </p>
      <div className="external-api-panel">
        <PreferenceToggle
          title={t("settings.system.externalApi.enabled")}
          description={t("settings.system.externalApi.enabledDescription")}
          checked={config.enabled}
          disabled={saveState === "saving"}
          onChange={(enabled) => onChange({ enabled })}
        />
        <div className="external-api-address-row">
          <div className="external-api-address-grid">
            <label className="field">
              <span>{t("settings.system.externalApi.host")}</span>
              <input
                key={config.host}
                defaultValue={config.host}
                disabled={saveState === "saving"}
                onBlur={(event) => {
                  const host = event.currentTarget.value.trim();
                  if (!host || host === config.host) return;
                  const loopback = host === "127.0.0.1" || host === "::1";
                  onChange({ host, require_token: loopback ? config.require_token : true });
                }}
              />
            </label>
            <label className="field">
              <span>{t("settings.system.externalApi.port")}</span>
              <input
                key={config.port}
                type="number"
                min={1}
                max={65535}
                defaultValue={config.port}
                disabled={saveState === "saving"}
                onBlur={(event) => {
                  const port = Number.parseInt(event.currentTarget.value, 10);
                  if (port >= 1 && port <= 65535 && port !== config.port) onChange({ port });
                }}
              />
            </label>
          </div>
        </div>
        <PreferenceToggle
          title={t("settings.system.externalApi.requireToken")}
          description={t("settings.system.externalApi.requireTokenDescription")}
          checked={config.require_token}
          disabled={saveState === "saving" || !["127.0.0.1", "::1"].includes(config.host)}
          onChange={(require_token) => onChange({ require_token })}
        />
        <div className="external-api-token-section">
          <div className="external-api-token-row">
            <span className="external-api-token-copy">
              <span className="external-api-token-title"><KeyRound size={15} aria-hidden="true" /><strong>{t("settings.system.externalApi.token")}</strong></span>
              <small className={`external-api-token-status ${tokenState}`}>{tokenStatusMessage}</small>
            </span>
            <div className="external-api-token-actions">
              {tokenStatus?.stored_configured && (
                <button className="secondary-button external-api-delete-button" type="button" disabled={tokenBusy || tokenStatus.environment_override} onClick={() => void deleteToken()}>
                  <Trash2 size={14} aria-hidden="true" />
                  {t("settings.system.externalApi.deleteToken")}
                </button>
              )}
              <button className="secondary-button" type="button" disabled={tokenBusy || tokenStatus?.environment_override} onClick={generateToken}>
                <RefreshCw size={14} aria-hidden="true" />
                {t("settings.system.externalApi.generateToken")}
              </button>
            </div>
          </div>
          <div className="external-api-token-input">
            <input
              type="password"
              value={tokenInput}
              placeholder={t("settings.system.externalApi.tokenInput")}
              aria-label={t("settings.system.externalApi.tokenInput")}
              disabled={tokenBusy || tokenStatus?.environment_override}
              onChange={(event) => setTokenInput(event.currentTarget.value)}
            />
            <button className="secondary-button" type="button" disabled={tokenBusy || tokenStatus?.environment_override || !tokenInput.trim()} onClick={() => void storeToken(tokenInput, false)}>
              <Save size={14} aria-hidden="true" />
              {t("settings.system.externalApi.saveToken")}
            </button>
          </div>
          {generatedToken && (
            <div className="external-api-generated-token">
              <code>{generatedToken}</code>
              <button className="secondary-button" type="button" onClick={() => void navigator.clipboard.writeText(generatedToken)}>
                <Copy size={14} aria-hidden="true" />
                {t("settings.system.externalApi.copyToken")}
              </button>
            </div>
          )}
          {tokenMessage && <p className="external-api-feedback" role="status" aria-live="polite">{tokenMessage}</p>}
        </div>
        <p className="external-api-restart"><Info size={14} aria-hidden="true" /><span>{t("settings.system.externalApi.restartRequired")}</span></p>
      </div>
    </div>
  );
}
