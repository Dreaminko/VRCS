import { useTranslation } from "react-i18next";
import { PlusCircle, RefreshCw } from "lucide-react";

import type { AnkiStatus } from "../../anki/types";
import type { Settings } from "../types";
import type { SaveState, SettingOption } from "../settings-types";
import { DeckTreeSelect, PreferenceToggle, Select } from "../SettingsControls";

export function AnkiSettingsSection({
  draft,
  status,
  busy,
  message,
  portText,
  portError,
  saveState,
  deckNames,
  modelOptions,
  frontFieldOptions,
  backFieldOptions,
  onLoadStatus,
  onSetPortText,
  onCommitPort,
  onUpdate,
}: {
  draft: Settings;
  status: AnkiStatus | null;
  busy: boolean;
  message: string;
  portText: string;
  portError: string;
  saveState: SaveState;
  deckNames: string[];
  modelOptions: SettingOption[];
  frontFieldOptions: SettingOption[];
  backFieldOptions: SettingOption[];
  onLoadStatus: () => Promise<void>;
  onSetPortText: (value: string) => void;
  onCommitPort: () => void;
  onUpdate: <K extends keyof Settings["anki"]>(key: K, value: Settings["anki"][K]) => void;
}) {
  const { t } = useTranslation();
  const ankiStatus = status;
  const ankiBusy = busy;
  const ankiMessage = message;
  const ankiPortText = portText;
  const ankiPortError = portError;
  const ankiDeckNames = deckNames;
  const ankiModelOptions = modelOptions;
  const ankiFieldOptions = frontFieldOptions;
  const ankiBackFieldOptions = backFieldOptions;
  const loadAnkiStatus = onLoadStatus;
  const setAnkiPortText = onSetPortText;
  const commitAnkiPort = onCommitPort;
  const updateAnki = onUpdate;
  return (
    <section className="connections-settings-group anki-section" aria-labelledby="connections-anki-title">
      <div className="section-heading">
        <div>
          <PlusCircle size={18} />
          <h3 id="connections-anki-title">{t("settings.anki.title")}</h3>
        </div>
        {draft.anki.enabled && (
          <button className="secondary-button" type="button" disabled={ankiBusy} onClick={() => void loadAnkiStatus()}>
            <RefreshCw className={ankiBusy ? "spin" : ""} size={15} />
            {ankiBusy ? t("common.checking") : t("common.checkAgain")}
          </button>
        )}
      </div>

      {draft.anki.enabled && (
        <div className={`anki-connection ${ankiBusy ? "checking" : ankiStatus?.connected ? (ankiStatus.configuration_valid ? "ready" : "needs-setup") : "offline"}`} role="status" aria-live="polite">
          <span className="anki-connection-dot" aria-hidden="true" />
          <div>
            <strong>
              {ankiBusy
                ? t("settings.anki.checking")
                : ankiStatus?.connected
                  ? ankiStatus.configuration_valid
                    ? t("settings.anki.ready")
                    : t("settings.anki.needsSetup")
                  : t("settings.anki.offline")}
            </strong>
            <p>{ankiMessage || t("settings.anki.startHint")}</p>
          </div>
          <code>
            {ankiStatus?.version ? `API v${ankiStatus.version}` : `127.0.0.1:${draft.anki.port}`}
          </code>
        </div>
      )}

      <div className={`anki-panel ${draft.anki.enabled ? "enabled" : ""}`}>
        <div className="settings-toggle-list settings-feature-toggle">
          <PreferenceToggle
            title={t("settings.anki.enable")}
            checked={draft.anki.enabled}
            disabled={saveState === "saving"}
            onChange={(enabled) => updateAnki("enabled", enabled)}
          />
        </div>

        {draft.anki.enabled && (
          <>
            <div className="anki-endpoint-row">
              <div className="anki-endpoint-field">
                <span>{t("settings.anki.address")}</span>
                <code className="anki-address-value">127.0.0.1</code>
              </div>
              <label className="anki-port-field">
                <span>{t("settings.anki.port")}</span>
                <input
                  type="text"
                  inputMode="numeric"
                  value={ankiPortText}
                  disabled={saveState === "saving"}
                  aria-invalid={Boolean(ankiPortError)}
                  aria-describedby={ankiPortError ? "anki-port-help" : undefined}
                  onChange={(event) => setAnkiPortText(event.target.value.replace(/\D/g, "").slice(0, 5))}
                  onBlur={commitAnkiPort}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      event.preventDefault();
                      event.currentTarget.blur();
                    }
                  }}
                />
                {ankiPortError && <small id="anki-port-help" className="error">{ankiPortError}</small>}
              </label>
            </div>

            <div className="form-grid anki-mapping-grid">
              <DeckTreeSelect
                label={t("settings.anki.deck")}
                helper={ankiStatus?.connected ? undefined : t("settings.anki.deckOffline")}
                value={draft.anki.deck}
                decks={ankiDeckNames}
                disabled={!ankiStatus?.connected || ankiBusy || saveState === "saving"}
                onChange={(value) => updateAnki("deck", value)}
              />
              <Select
                label={t("settings.anki.noteType")}
                helper={ankiStatus?.connected ? undefined : t("settings.anki.noteTypeOffline")}
                value={draft.anki.model}
                options={ankiModelOptions}
                disabled={!ankiStatus?.connected || ankiBusy || saveState === "saving"}
                onChange={(value) => updateAnki("model", value)}
              />
              <Select
                label={t("settings.anki.frontField")}
                value={draft.anki.front_field}
                options={ankiFieldOptions}
                disabled={!ankiStatus?.connected || !ankiStatus.fields.length || ankiBusy || saveState === "saving"}
                onChange={(value) => updateAnki("front_field", value)}
              />
              <Select
                label={t("settings.anki.backField")}
                value={draft.anki.back_field}
                options={ankiBackFieldOptions}
                disabled={!ankiStatus?.connected || !ankiStatus.fields.length || ankiBusy || saveState === "saving"}
                onChange={(value) => updateAnki("back_field", value)}
              />
            </div>
          </>
        )}
      </div>
    </section>
  );
}
