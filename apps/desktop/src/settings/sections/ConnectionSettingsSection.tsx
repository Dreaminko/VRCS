import { useEffect, useState } from "react";
import { Link, RadioTower, Send } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { AnkiStatus, Health, Settings } from "../../types";
import type { ApplySettings, SaveState, SettingOption } from "../settings-types";
import { PreferenceToggle, Select } from "../SettingsControls";
import { ExternalApiSettingsCard } from "../system/ExternalApiSettingsCard";
import { VrcxIntegrationSettingsCard } from "../system/VrcxIntegrationSettingsCard";
import { AnkiSettingsSection } from "./AnkiSettingsSection";

export function ConnectionSettingsSection({
  draft,
  health,
  saveState,
  applySettings,
  onTest,
  anki,
}: {
  draft: Settings;
  health: Health | null;
  saveState: SaveState;
  applySettings: ApplySettings;
  onTest: () => Promise<void>;
  anki: {
    status: AnkiStatus | null;
    busy: boolean;
    message: string;
    portText: string;
    portError: string;
    deckNames: string[];
    modelOptions: SettingOption[];
    frontFieldOptions: SettingOption[];
    backFieldOptions: SettingOption[];
    onLoadStatus: () => Promise<void>;
    onSetPortText: (value: string) => void;
    onCommitPort: () => void;
    onUpdate: <K extends keyof Settings["anki"]>(key: K, value: Settings["anki"][K]) => void;
  };
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
  const updateVrcx = (patch: Partial<Settings["vrcx"]>) => {
    applySettings((current) => ({
      ...current,
      vrcx: { ...current.vrcx, ...patch },
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
              <div className="osc-strategy-setting">
                <strong>{t("settings.osc.translationStrategy")}</strong>
                <Select
                  label={t("settings.osc.translationStrategy")}
                  hideLabel
                  value={draft.osc.translation_strategy}
                  disabled={saveState === "saving"}
                  options={[
                    { value: "preferred_only", label: t("settings.osc.translationStrategies.preferredOnly") },
                    { value: "round_robin", label: t("settings.osc.translationStrategies.roundRobin") },
                    { value: "all_languages", label: t("settings.osc.translationStrategies.allLanguages") },
                  ]}
                  onChange={(translation_strategy) => applySettings((current) => ({
                    ...current,
                    osc: {
                      ...current.osc,
                      translation_strategy: translation_strategy as Settings["osc"]["translation_strategy"],
                    },
                  }))}
                />
              </div>
              <PreferenceToggle
                title={t("settings.osc.preserveOriginal")}
                description={t("settings.osc.preserveOriginalDescription")}
                checked={draft.osc.preserve_original_text}
                disabled={saveState === "saving"}
                onChange={(preserve_original_text) => applySettings((current) => ({
                  ...current,
                  osc: { ...current.osc, preserve_original_text },
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
        <AnkiSettingsSection
          draft={draft}
          status={anki.status}
          busy={anki.busy}
          message={anki.message}
          portText={anki.portText}
          portError={anki.portError}
          saveState={saveState}
          deckNames={anki.deckNames}
          modelOptions={anki.modelOptions}
          frontFieldOptions={anki.frontFieldOptions}
          backFieldOptions={anki.backFieldOptions}
          onLoadStatus={anki.onLoadStatus}
          onSetPortText={anki.onSetPortText}
          onCommitPort={anki.onCommitPort}
          onUpdate={anki.onUpdate}
        />
        <VrcxIntegrationSettingsCard
          config={draft.vrcx}
          saveState={saveState}
          onChange={updateVrcx}
        />
        <ExternalApiSettingsCard
          config={draft.external_api}
          saveState={saveState}
          onChange={updateExternalApi}
        />
      </div>
    </div>
  );
}
