import { KeyRound } from "lucide-react";
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
  const [tokenBusy, setTokenBusy] = useState(false);
  const [tokenMessage, setTokenMessage] = useState("");
  const [generatedToken, setGeneratedToken] = useState("");
  const [tokenInput, setTokenInput] = useState("");

  useEffect(() => {
    let cancelled = false;
    void coreApi.externalApiTokenStatus().then(
      (status) => {
        if (!cancelled) setTokenStatus(status);
      },
      () => {
        if (!cancelled) setTokenMessage(t("settings.system.externalApi.tokenLoadFailed"));
      },
    );
    void coreApi.externalApiRuntimeStatus().then(
      (status) => {
        if (!cancelled) setRuntimeStatus(status);
      },
      () => {
        if (!cancelled) setTokenMessage(t("settings.system.externalApi.statusLoadFailed"));
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

  return (
    <div className="external-api-settings">
      <div className="external-api-heading">
        <div><KeyRound size={17} /><strong>{t("settings.system.externalApi.title")}</strong></div>
        <small>{t("settings.system.externalApi.description")}</small>
      </div>
      {runtimeStatus && (
        <p className={`external-api-runtime-status ${runtimeStatus.state}`}>
          {t(`settings.system.externalApi.runtime.${runtimeStatus.state}`, {
            address: runtimeStatus.address ?? "",
            error: runtimeStatus.error ?? "",
          })}
        </p>
      )}
      <PreferenceToggle
        title={t("settings.system.externalApi.enabled")}
        description={t("settings.system.externalApi.enabledDescription")}
        checked={config.enabled}
        disabled={saveState === "saving"}
        onChange={(enabled) => onChange({ enabled })}
      />
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
      <PreferenceToggle
        title={t("settings.system.externalApi.requireToken")}
        description={t("settings.system.externalApi.requireTokenDescription")}
        checked={config.require_token}
        disabled={saveState === "saving" || !["127.0.0.1", "::1"].includes(config.host)}
        onChange={(require_token) => onChange({ require_token })}
      />
      <div className="external-api-token-row">
        <span>
          <strong>{t("settings.system.externalApi.token")}</strong>
          <small>{tokenStatus?.environment_override
            ? t("settings.system.externalApi.tokenEnvironment")
            : tokenStatus?.configured
              ? t("settings.system.externalApi.tokenConfigured")
              : t("settings.system.externalApi.tokenMissing")}</small>
        </span>
        <div>
          {tokenStatus?.stored_configured && (
            <button className="secondary-button" type="button" disabled={tokenBusy || tokenStatus.environment_override} onClick={() => void deleteToken()}>
              {t("settings.system.externalApi.deleteToken")}
            </button>
          )}
          <button className="secondary-button" type="button" disabled={tokenBusy || tokenStatus?.environment_override} onClick={generateToken}>
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
          {t("settings.system.externalApi.saveToken")}
        </button>
      </div>
      {generatedToken && (
        <div className="external-api-generated-token">
          <code>{generatedToken}</code>
          <button className="secondary-button" type="button" onClick={() => void navigator.clipboard.writeText(generatedToken)}>
            {t("settings.system.externalApi.copyToken")}
          </button>
        </div>
      )}
      {tokenMessage && <p className="external-api-feedback">{tokenMessage}</p>}
      <p className="external-api-restart">{t("settings.system.externalApi.restartRequired")}</p>
    </div>
  );
}
