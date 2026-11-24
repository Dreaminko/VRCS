import { invoke, isTauri } from "@tauri-apps/api/core";
import { ExternalLink, Monitor, Send, Trash2 } from "lucide-react";
import { useEffect, useState, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";

import { integrationsApi } from "../../integrations/api";
import type { UniTextRuntimeStatus, UniTextSettings, UniTextTier } from "../../integrations/types";
import type { SaveState } from "../settings-types";
import { PreferenceToggle, Select } from "../SettingsControls";
import { SettingsDisclosure } from "../components/SettingsDisclosure";

const UNITEXT_REPOSITORY_URL = "https://github.com/Dreaminko/UniText";

export function UniTextSettingsCard({
  config,
  runtime,
  saveState,
  onChange,
}: {
  config: UniTextSettings;
  runtime: UniTextRuntimeStatus | undefined;
  saveState: SaveState;
  onChange: (patch: Partial<UniTextSettings>) => void;
}) {
  const { t } = useTranslation();
  const [portText, setPortText] = useState(String(config.port));
  const [portError, setPortError] = useState("");
  const [intervalText, setIntervalText] = useState(String(config.interval_ms));
  const [intervalError, setIntervalError] = useState("");
  const [clearAfterText, setClearAfterText] = useState(String(config.clear_after_seconds));
  const [clearAfterError, setClearAfterError] = useState("");
  const [busyAction, setBusyAction] = useState<"test" | "clear" | null>(null);
  const [feedback, setFeedback] = useState("");

  useEffect(() => setPortText(String(config.port)), [config.port]);
  useEffect(() => setIntervalText(String(config.interval_ms)), [config.interval_ms]);
  useEffect(() => setClearAfterText(String(config.clear_after_seconds)), [config.clear_after_seconds]);

  const commitPort = () => {
    const port = Number(portText);
    if (!Number.isInteger(port) || port < 1 || port > 65_535) {
      setPortError(t("settings.unitext.invalidPort"));
      return;
    }
    setPortError("");
    onChange({ port });
  };

  const commitInterval = () => {
    const interval_ms = Number(intervalText);
    if (!Number.isInteger(interval_ms) || interval_ms < 80 || interval_ms > 500) {
      setIntervalError(t("settings.unitext.invalidInterval"));
      return;
    }
    setIntervalError("");
    onChange({ interval_ms });
  };

  const commitClearAfter = () => {
    const clear_after_seconds = Number(clearAfterText);
    if (!Number.isInteger(clear_after_seconds) || clear_after_seconds < 0 || clear_after_seconds > 300) {
      setClearAfterError(t("settings.unitext.invalidClearAfter"));
      return;
    }
    setClearAfterError("");
    onChange({ clear_after_seconds });
  };

  const handleTest = async () => {
    setBusyAction("test");
    setFeedback("");
    try {
      await integrationsApi.testUniText();
      setFeedback(t("settings.unitext.testQueued"));
    } catch {
      setFeedback(t("settings.unitext.testFailed"));
    } finally {
      setBusyAction(null);
    }
  };

  const handleClear = async () => {
    setBusyAction("clear");
    setFeedback("");
    try {
      await integrationsApi.clearUniText();
      setFeedback(t("settings.unitext.cleared"));
    } catch {
      setFeedback(t("settings.unitext.clearFailed"));
    } finally {
      setBusyAction(null);
    }
  };

  const openRepository = (event: MouseEvent<HTMLAnchorElement>) => {
    if (!isTauri()) return;
    event.preventDefault();
    void invoke("open_project_link", { target: UNITEXT_REPOSITORY_URL }).catch((error) => {
      console.error("Failed to open UniText repository", error);
    });
  };

  const statusKey = !config.enabled
    ? "disabled"
    : runtime?.status ?? "discovering";
  const statusClass = statusKey === "error"
    ? "error"
    : statusKey === "ready"
      ? "ready"
      : "disabled";
  const activeSummary = runtime?.active_tier
    ? t(`settings.unitext.tiers.${runtime.active_tier}`)
    : null;
  const statusDetail = runtime?.last_error
    || feedback
    || activeSummary
    || t("settings.unitext.statusHint");

  return (
    <section className="connections-settings-group osc-section" aria-labelledby="connections-unitext-title">
      <div className="section-heading connections-feature-heading">
        <div>
          <Monitor size={18} />
          <div className="connections-heading-title">
            <h3 id="connections-unitext-title">{t("settings.unitext.title")}</h3>
            <a
              className="connections-title-link-button"
              href={UNITEXT_REPOSITORY_URL}
              target="_blank"
              rel="noopener noreferrer"
              aria-label={t("settings.unitext.openRepository")}
              data-tooltip={t("settings.unitext.openRepository")}
              onClick={openRepository}
            >
              <ExternalLink size={14} aria-hidden="true" />
            </a>
          </div>
        </div>
        <div className="connections-heading-actions">
          {config.enabled && (
            <>
              <button
                className="secondary-button"
                type="button"
                disabled={busyAction !== null || saveState === "saving"}
                onClick={() => void handleTest()}
              >
                <Send size={15} />
                {busyAction === "test" ? t("settings.unitext.testing") : t("settings.unitext.test")}
              </button>
              <button
                className="secondary-button"
                type="button"
                disabled={busyAction !== null || saveState === "saving"}
                onClick={() => void handleClear()}
              >
                <Trash2 size={15} />
                {t("settings.unitext.clear")}
              </button>
            </>
          )}
          <PreferenceToggle
            title={t("settings.unitext.enable")}
            hideLabel
            checked={config.enabled}
            disabled={false}
            onChange={(enabled) => onChange({ enabled })}
          />
        </div>
      </div>

      <SettingsDisclosure open={config.enabled}>
        <div className={`osc-connection ${statusClass}`} aria-live="polite">
          <span className="osc-connection-dot" aria-hidden="true" />
          <div>
            <strong>{t(`settings.unitext.status.${statusKey}`)}</strong>
            <p>{statusDetail}</p>
          </div>
          <code>{runtime?.target || `127.0.0.1:${config.port}`}</code>
        </div>
      </SettingsDisclosure>

      <SettingsDisclosure open={config.enabled}>
        <div className="osc-panel enabled">
          <div className="settings-toggle-list settings-feature-toggle">
            <div className="osc-strategy-setting">
              <strong>{t("settings.unitext.tier")}</strong>
              <Select
                label={t("settings.unitext.tier")}
                hideLabel
                value={config.tier}
                disabled={false}
                options={[
                  { value: "auto", label: t("settings.unitext.tiers.auto") },
                  { value: "lite", label: t("settings.unitext.tiers.lite") },
                  { value: "compact", label: t("settings.unitext.tiers.compact") },
                  { value: "balanced", label: t("settings.unitext.tiers.balanced") },
                  { value: "standard", label: t("settings.unitext.tiers.standard") },
                  { value: "turbo", label: t("settings.unitext.tiers.turbo") },
                ]}
                onChange={(tier) => onChange({ tier: tier as UniTextTier })}
              />
            </div>
            <PreferenceToggle
              title={t("settings.unitext.includeMicrophone")}
              checked={config.include_microphone}
              disabled={false}
              onChange={(include_microphone) => onChange({ include_microphone })}
            />
            <PreferenceToggle
              title={t("settings.unitext.muteSync")}
              checked={config.mute_sync_enabled}
              disabled={false}
              onChange={(mute_sync_enabled) => onChange({ mute_sync_enabled })}
            />
            <PreferenceToggle
              title={t("settings.unitext.includeSpeaker")}
              description={t("settings.unitext.includeSpeakerDescription")}
              checked={config.include_speaker}
              disabled={false}
              onChange={(include_speaker) => onChange({ include_speaker })}
            />
            <PreferenceToggle
              title={t("settings.unitext.preserveOriginal")}
              description={t("settings.unitext.preserveOriginalDescription")}
              checked={config.preserve_original_text}
              disabled={false}
              onChange={(preserve_original_text) => onChange({ preserve_original_text })}
            />
          </div>

          <div className="osc-endpoint-row">
            <div className="osc-endpoint-field">
              <span>{t("settings.unitext.address")}</span>
              <code className="osc-address-value">127.0.0.1</code>
            </div>
            <label className="osc-endpoint-field osc-port-field">
              <span>{t("settings.unitext.port")}</span>
              <input
                type="text"
                inputMode="numeric"
                value={portText}
                aria-invalid={Boolean(portError)}
                aria-describedby={portError ? "unitext-port-help" : undefined}
                onChange={(event) => setPortText(event.target.value.replace(/\D/g, "").slice(0, 5))}
                onBlur={commitPort}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    event.currentTarget.blur();
                  }
                }}
              />
              {portError && <small id="unitext-port-help" className="error">{portError}</small>}
            </label>
          </div>

          <div className="osc-endpoint-row">
            <label className="osc-endpoint-field osc-port-field">
              <span>{t("settings.unitext.intervalMs")}</span>
              <input
                type="text"
                inputMode="numeric"
                value={intervalText}
                aria-invalid={Boolean(intervalError)}
                aria-describedby={intervalError ? "unitext-interval-help" : undefined}
                onChange={(event) => setIntervalText(event.target.value.replace(/\D/g, "").slice(0, 3))}
                onBlur={commitInterval}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    event.currentTarget.blur();
                  }
                }}
              />
              {intervalError && <small id="unitext-interval-help" className="error">{intervalError}</small>}
            </label>
            <label className="osc-endpoint-field osc-port-field">
              <span>{t("settings.unitext.clearAfterSeconds")}</span>
              <input
                type="text"
                inputMode="numeric"
                value={clearAfterText}
                aria-invalid={Boolean(clearAfterError)}
                aria-describedby={clearAfterError ? "unitext-clear-help" : undefined}
                onChange={(event) => setClearAfterText(event.target.value.replace(/\D/g, "").slice(0, 3))}
                onBlur={commitClearAfter}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    event.currentTarget.blur();
                  }
                }}
              />
              {clearAfterError && <small id="unitext-clear-help" className="error">{clearAfterError}</small>}
            </label>
          </div>
        </div>
      </SettingsDisclosure>
    </section>
  );
}
