import { KeyRound, RadioTower, RefreshCw, Save, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { coreApi } from "../../api";
import type { CredentialStatus, Settings, VrcxRuntimeStatus } from "../../types";
import type { SaveState } from "../settings-types";
import { PreferenceToggle } from "../SettingsControls";

export function VrcxIntegrationSettingsCard({ config, saveState, onChange }: {
  config: Settings["vrcx"];
  saveState: SaveState;
  onChange: (patch: Partial<Settings["vrcx"]>) => void;
}) {
  const { t } = useTranslation();
  const [tokenStatus, setTokenStatus] = useState<CredentialStatus | null>(null);
  const [runtimeStatus, setRuntimeStatus] = useState<VrcxRuntimeStatus | null>(null);
  const [tokenStatusError, setTokenStatusError] = useState(false);
  const [runtimeStatusError, setRuntimeStatusError] = useState(false);
  const [tokenBusy, setTokenBusy] = useState(false);
  const [testing, setTesting] = useState(false);
  const [tokenInput, setTokenInput] = useState("");
  const [tokenMessage, setTokenMessage] = useState("");
  const [testMessage, setTestMessage] = useState("");
  const [portText, setPortText] = useState(String(config.port));
  const [portError, setPortError] = useState("");

  useEffect(() => setPortText(String(config.port)), [config.port]);

  useEffect(() => {
    let cancelled = false;
    void coreApi.vrcxTokenStatus().then(
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
    void coreApi.vrcxRuntimeStatus().then(
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
  }, []);

  useEffect(() => {
    if (saveState !== "saved") return;
    let cancelled = false;
    void coreApi.vrcxRuntimeStatus().then(
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
  }, [saveState]);

  useEffect(() => {
    if (!config.enabled) return;
    const timer = window.setInterval(() => {
      void coreApi.vrcxRuntimeStatus().then(
        (status) => {
          setRuntimeStatus(status);
          setRuntimeStatusError(false);
        },
        () => setRuntimeStatusError(true),
      );
    }, 3_000);
    return () => window.clearInterval(timer);
  }, [config.enabled]);

  const refreshRuntimeStatus = () => {
    void coreApi.vrcxRuntimeStatus().then(
      (status) => {
        setRuntimeStatus(status);
        setRuntimeStatusError(false);
      },
      () => setRuntimeStatusError(true),
    );
  };

  const commitPort = () => {
    const port = Number(portText);
    if (!Number.isInteger(port) || port < 1 || port > 65_535) {
      setPortError(t("settings.vrcx.invalidPort"));
      return;
    }
    setPortError("");
    if (port !== config.port) onChange({ port });
  };

  const saveToken = async () => {
    const token = tokenInput.trim();
    if (!token) return;
    setTokenBusy(true);
    setTokenMessage("");
    try {
      setTokenStatus(await coreApi.saveVrcxToken(token));
      setTokenStatusError(false);
      setTokenInput("");
      setTokenMessage(t("settings.vrcx.tokenSaved"));
      refreshRuntimeStatus();
    } catch {
      setTokenMessage(t("settings.vrcx.tokenSaveFailed"));
    } finally {
      setTokenBusy(false);
    }
  };

  const deleteToken = async () => {
    setTokenBusy(true);
    setTokenMessage("");
    try {
      setTokenStatus(await coreApi.deleteVrcxToken());
      setTokenStatusError(false);
      setTokenMessage(t("settings.vrcx.tokenDeleted"));
      refreshRuntimeStatus();
    } catch {
      setTokenMessage(t("settings.vrcx.tokenDeleteFailed"));
    } finally {
      setTokenBusy(false);
    }
  };

  const testConnection = async () => {
    setTesting(true);
    setTestMessage("");
    try {
      setRuntimeStatus(await coreApi.testVrcx());
      setRuntimeStatusError(false);
    } catch {
      setTestMessage(t("settings.vrcx.testFailed"));
    } finally {
      setTesting(false);
    }
  };

  const runtimeState = runtimeStatusError ? "load_failed" : runtimeStatus?.state ?? "checking";
  const runtimeClass = runtimeState === "connected"
    ? "running"
    : runtimeState === "error" || runtimeState === "load_failed"
      ? "failed"
      : runtimeState === "connecting" || runtimeState === "checking"
        ? "checking"
        : runtimeState;
  const runtimeDetail = runtimeStatus?.state === "connected"
    ? runtimeStatus.world_name
      ? t("settings.vrcx.worldSummary", { world: runtimeStatus.world_name, count: runtimeStatus.member_count })
      : t("settings.vrcx.noWorldSummary", { count: runtimeStatus.member_count })
    : runtimeStatus?.state === "error" && runtimeStatus.error
      ? runtimeStatus.error
      : testMessage;
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
    ? t("settings.vrcx.tokenLoadFailed")
    : tokenStatus === null
      ? t("settings.vrcx.tokenChecking")
      : tokenStatus.environment_override
        ? t("settings.vrcx.tokenEnvironment")
        : tokenStatus.configured
          ? t("settings.vrcx.tokenConfigured")
          : t("settings.vrcx.tokenMissing");

  return (
    <section className="connections-settings-group external-api-settings vrcx-settings" aria-labelledby="connections-vrcx-title">
      <div className="section-heading">
        <div>
          <RadioTower size={18} />
          <h3 id="connections-vrcx-title">{t("settings.vrcx.title")}</h3>
        </div>
        {config.enabled && (
          <button className="secondary-button" type="button" disabled={testing || tokenBusy || saveState === "saving"} onClick={() => void testConnection()}>
            <RefreshCw className={testing ? "spin" : undefined} size={14} aria-hidden="true" />
            {testing ? t("settings.vrcx.testing") : t("settings.vrcx.test")}
          </button>
        )}
      </div>

      <div className={`external-api-runtime-status ${runtimeClass}`} role="status" aria-live="polite">
        <span className="external-api-status-dot" aria-hidden="true" />
        <span className="vrcx-runtime-copy">
          <strong>{t(`settings.vrcx.runtime.${runtimeState}`)}</strong>
          {runtimeDetail && <small>{runtimeDetail}</small>}
        </span>
      </div>

      <div className="external-api-panel">
        <PreferenceToggle
          title={t("settings.vrcx.enabled")}
          checked={config.enabled}
          disabled={saveState === "saving"}
          onChange={(enabled) => onChange({ enabled })}
        />
        <div className="external-api-address-row">
          <div className="external-api-address-grid vrcx-port-grid">
            <label className="field">
              <span>{t("settings.vrcx.port")}</span>
              <input
                type="text"
                inputMode="numeric"
                value={portText}
                disabled={saveState === "saving"}
                aria-invalid={Boolean(portError)}
                aria-describedby={portError ? "vrcx-port-help" : undefined}
                onChange={(event) => setPortText(event.currentTarget.value.replace(/\D/g, "").slice(0, 5))}
                onBlur={commitPort}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    event.currentTarget.blur();
                  }
                }}
              />
              <small id="vrcx-port-help" className={portError ? "error" : undefined}>{portError || t("settings.vrcx.portHint")}</small>
            </label>
          </div>
        </div>
        <PreferenceToggle
          title={t("settings.vrcx.includeInLlmContext")}
          checked={config.include_in_llm_context}
          disabled={saveState === "saving"}
          onChange={(include_in_llm_context) => onChange({ include_in_llm_context })}
        />
        <PreferenceToggle
          title={t("settings.vrcx.includeInAsrContext")}
          checked={config.include_in_asr_context}
          disabled={saveState === "saving"}
          onChange={(include_in_asr_context) => onChange({ include_in_asr_context })}
        />

        <div className="external-api-token-section">
          <div className="external-api-token-row">
            <span className="external-api-token-copy">
              <span className="external-api-token-title"><KeyRound size={15} aria-hidden="true" /><strong>{t("settings.vrcx.token")}</strong></span>
              <small className={`external-api-token-status ${tokenState}`}>{tokenStatusMessage}</small>
            </span>
            {tokenStatus?.stored_configured && (
              <div className="external-api-token-actions">
                <button className="secondary-button external-api-delete-button" type="button" disabled={tokenBusy || tokenStatus.environment_override} onClick={() => void deleteToken()}>
                  <Trash2 size={14} aria-hidden="true" />
                  {t("settings.vrcx.deleteToken")}
                </button>
              </div>
            )}
          </div>
          <p className="external-api-feedback">{t("settings.vrcx.tokenManagedHint")}</p>
          <div className="external-api-token-input">
            <input
              type="password"
              value={tokenInput}
              autoComplete="off"
              spellCheck={false}
              placeholder={t("settings.vrcx.tokenInput")}
              aria-label={t("settings.vrcx.tokenInput")}
              disabled={tokenBusy || tokenStatus?.environment_override}
              onChange={(event) => setTokenInput(event.currentTarget.value)}
            />
            <button className="secondary-button" type="button" disabled={tokenBusy || tokenStatus?.environment_override || !tokenInput.trim()} onClick={() => void saveToken()}>
              <Save size={14} aria-hidden="true" />
              {t("settings.vrcx.saveToken")}
            </button>
          </div>
          {tokenMessage && <p className="external-api-feedback" role="status" aria-live="polite">{tokenMessage}</p>}
        </div>
      </div>
    </section>
  );
}
