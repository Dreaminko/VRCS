import { useEffect, useState } from "react";
import { Link, RadioTower, Send } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { Health, Settings } from "../../types";
import type { ApplySettings, SaveState } from "../settings-types";
import { PreferenceToggle } from "../SettingsControls";
import { ExternalApiSettingsCard } from "../system/ExternalApiSettingsCard";

export function ConnectionSettingsSection({
  draft,
  health,
  saveState,
  applySettings,
  onTest,
}: {
  draft: Settings;
  health: Health | null;
  saveState: SaveState;
  applySettings: ApplySettings;
  onTest: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const [portText, setPortText] = useState(String(draft.osc.port));
  const [portError, setPortError] = useState("");
  const [testing, setTesting] = useState(false);
  const [message, setMessage] = useState("");

  useEffect(() => setPortText(String(draft.osc.port)), [draft.osc.port]);

  const commitPort = () => {
    const port = Number(portText);
    if (!Number.isInteger(port) || port < 1 || port > 65_535) {
      setPortError(t("settings.osc.invalidPort"));
      return;
    }
    setPortError("");
    applySettings((current) => ({
      ...current,
      osc: { ...current.osc, port },
    }));
  };

  const test = async () => {
    setTesting(true);
    setMessage("");
    try {
      await onTest();
      setMessage(t("settings.osc.testQueued"));
    } catch {
      setMessage(t("settings.osc.testFailed"));
    } finally {
      setTesting(false);
    }
  };

  const runtime = health?.osc;
  const state = !draft.osc.enabled
    ? "disabled"
    : runtime?.status === "error"
      ? "error"
      : "ready";
  const statusKey = state === "error"
    ? "error"
    : runtime?.send_gate === "blocked_vrchat_muted"
      ? "muted"
      : runtime?.send_gate === "blocked_mute_unknown"
        ? "unknown"
        : state;
  const statusDetail = runtime?.last_error
    || (runtime?.send_gate === "blocked_mute_unknown" ? health?.vrchat_mute_sync?.last_error : null)
    || message
    || t("settings.osc.statusHint");
  const updateExternalApi = (patch: Partial<Settings["external_api"]>) => {
    applySettings((current) => ({
      ...current,
      external_api: { ...current.external_api, ...patch },
    }));
  };

  return (
    <div className="settings-section settings-section-active connections-section" id="settings-panel-connections" role="tabpanel" aria-labelledby="settings-tab-connections">
      <div className="section-heading connections-page-heading">
        <div>
          <Link size={18} />
          <h2>{t("settings.connections.title")}</h2>
        </div>
      </div>
      <div className="connections-settings-list">
        <section className="connections-settings-group osc-section" aria-labelledby="connections-osc-title">
          <div className="section-heading">
            <div>
              <RadioTower size={18} />
              <h3 id="connections-osc-title">{t("settings.osc.title")}</h3>
            </div>
            {draft.osc.enabled && (
              <button className="secondary-button" type="button" disabled={testing || saveState === "saving"} onClick={() => void test()}>
                <Send size={15} />
                {testing ? t("settings.osc.testing") : t("settings.osc.test")}
              </button>
            )}
          </div>

          {draft.osc.enabled && (
            <div className={`osc-connection ${state}`} aria-live="polite">
              <span className="osc-connection-dot" aria-hidden="true" />
              <div>
                <strong>{t(`settings.osc.status.${statusKey}`)}</strong>
                <p>{statusDetail}</p>
              </div>
              <code>{runtime?.target || `127.0.0.1:${draft.osc.port}`}</code>
            </div>
          )}

          <div className={`osc-panel ${draft.osc.enabled ? "enabled" : ""}`}>
            <div className="settings-toggle-list settings-feature-toggle">
              <PreferenceToggle
                title={t("settings.osc.enable")}
                checked={draft.osc.enabled}
                disabled={saveState === "saving"}
                onChange={(enabled) => applySettings((current) => ({
                  ...current,
                  osc: { ...current.osc, enabled },
                }))}
              />
              <PreferenceToggle
                title={t("settings.osc.muteSync")}
                checked={draft.osc.mute_sync_enabled}
                disabled={saveState === "saving"}
                onChange={(mute_sync_enabled) => applySettings((current) => ({
                  ...current,
                  osc: { ...current.osc, mute_sync_enabled },
                }))}
              />
              <PreferenceToggle
                title={t("settings.osc.muteToast")}
                checked={draft.osc.mute_status_toast_enabled}
                disabled={saveState === "saving"}
                onChange={(mute_status_toast_enabled) => applySettings((current) => ({
                  ...current,
                  osc: { ...current.osc, mute_status_toast_enabled },
                }))}
              />
            </div>

            {draft.osc.enabled && (
              <div className="osc-endpoint-row">
                <div className="osc-endpoint-field">
                  <span>{t("settings.osc.address")}</span>
                  <code className="osc-address-value">127.0.0.1</code>
                </div>
                <label className="osc-endpoint-field osc-port-field">
                  <span>{t("settings.osc.port")}</span>
                  <input
                    type="text"
                    inputMode="numeric"
                    value={portText}
                    disabled={saveState === "saving"}
                    aria-invalid={Boolean(portError)}
                    aria-describedby={portError ? "osc-port-help" : undefined}
                    onChange={(event) => setPortText(event.target.value.replace(/\D/g, "").slice(0, 5))}
                    onBlur={commitPort}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        event.preventDefault();
                        event.currentTarget.blur();
                      }
                    }}
                  />
                  {portError && <small id="osc-port-help" className="error">{portError}</small>}
                </label>
              </div>
            )}
          </div>
        </section>
        <ExternalApiSettingsCard
          config={draft.external_api}
          saveState={saveState}
          onChange={updateExternalApi}
        />
      </div>
    </div>
  );
}
