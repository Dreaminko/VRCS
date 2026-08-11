import { useEffect, useState } from "react";
import { RadioTower, Send } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { Health, Settings } from "../../types";
import type { ApplySettings, SaveState } from "../settings-types";
import { PreferenceToggle } from "../SettingsControls";

export function OscSettingsSection({
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

  return (
    <div className="settings-section settings-section-active osc-section" id="settings-panel-osc" role="tabpanel" aria-labelledby="settings-tab-osc">
      <div className="section-heading">
        <div>
          <RadioTower size={18} />
          <h2>{t("settings.osc.title")}</h2>
          <span>{t("settings.osc.subtitle")}</span>
        </div>
        {draft.osc.enabled && (
          <button className="secondary-button" type="button" disabled={testing || saveState === "saving"} onClick={() => void test()}>
            <Send size={15} />
            {testing ? t("settings.osc.testing") : t("settings.osc.test")}
          </button>
        )}
      </div>

      <div className="settings-toggle-list settings-feature-toggle">
        <PreferenceToggle
          title={t("settings.osc.enable")}
          description={t("settings.osc.enableDescription")}
          checked={draft.osc.enabled}
          disabled={saveState === "saving"}
          onChange={(enabled) => applySettings((current) => ({
            ...current,
            osc: { ...current.osc, enabled },
          }))}
        />
      </div>

      {draft.osc.enabled && <>
        <div className={`osc-connection ${state}`} aria-live="polite">
          <span className="osc-connection-dot" aria-hidden="true" />
          <div>
            <strong>{t(`settings.osc.status.${state}`)}</strong>
            <p>{runtime?.last_error || message || t("settings.osc.statusHint")}</p>
          </div>
          <code>{runtime?.target || `127.0.0.1:${draft.osc.port}`}</code>
        </div>

        <div className="osc-endpoint-row">
          <div className="osc-endpoint-field">
            <span>{t("settings.osc.address")}</span>
            <code className="osc-address-value">127.0.0.1</code>
            <small>{t("settings.osc.addressDescription")}</small>
          </div>
          <label className="osc-endpoint-field osc-port-field">
            <span>{t("settings.osc.port")}</span>
            <input
              type="text"
              inputMode="numeric"
              value={portText}
              disabled={saveState === "saving"}
              aria-invalid={Boolean(portError)}
              aria-describedby="osc-port-help"
              onChange={(event) => setPortText(event.target.value.replace(/\D/g, "").slice(0, 5))}
              onBlur={commitPort}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  event.currentTarget.blur();
                }
              }}
            />
            <small id="osc-port-help" className={portError ? "error" : ""}>
              {portError || t("settings.osc.portHint")}
            </small>
          </label>
        </div>
        <p className="osc-vrchat-hint">{t("settings.osc.vrchatHint")}</p>
      </>}
    </div>
  );
}
