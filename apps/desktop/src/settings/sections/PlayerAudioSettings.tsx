import { useState } from "react";
import { CircleHelp, FlaskConical } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { PlayerAudioSettings as Options } from "../../capture/types";
import { MemoryCompressionControls } from "../../capture/MemoryCompressionControls";
import { PlayerAudioStartDialog } from "../../capture/PlayerAudioStartDialog";
import { playerAudioDisclaimer } from "../../capture/player-audio-disclaimer";
import type { PlayerAudioStart } from "../../capture/usePlayerAudioStart";
import { usePlatformCapabilities } from "../../core-client/capabilities";
import type { PlayerAudioStatus } from "../../core-client/types";
import { EditableDropdownField } from "../../shared/ui/DropdownField";
import { PreferenceToggle, RangeField, Select } from "../SettingsControls";

export const DEFAULT_PLAYER_AUDIO: Options = {
  enabled: false, pagefile_drive: "C:", opus_library_path: "", max_players: 0, max_visible_players: 4,
  proximity: { enabled: false, radius_m: 10 }, view_priority: { enabled: false, horizontal_fov_deg: 120 },
};

export function PlayerAudioSettings({ value, status, memory, disabled, showUnsupportedNotice = true, onChange }: {
  value?: Options;
  status?: PlayerAudioStatus | null;
  memory?: PlayerAudioStart["memory"];
  disabled: boolean;
  showUnsupportedNotice?: boolean;
  onChange: (patch: Partial<Options>) => void;
}) {
  const { t } = useTranslation();
  const capabilities = usePlatformCapabilities();
  const os = capabilities.desktop?.os ?? capabilities.core?.os;
  const linux = capabilities.core?.os === "linux";
  const [disclaimerOpen, setDisclaimerOpen] = useState(false);
  const disclaimerTitle = playerAudioDisclaimer(t, os).title;
  const options = value ?? DEFAULT_PLAYER_AUDIO;
  const fieldsDisabled = disabled || !options.enabled;
  const phase = status?.phase ?? "stopped";
  const keyScan = status?.key_scan;
  const keyReady = keyScan?.stage === "verified" || keyScan?.stage === "cached";
  const formatPlayerCount = (count: number) => count === 0 ? t("settings.audio.players.unlimited") : String(count);
  const noVoiceSources = options.source_mode !== "everyone"
    && !Object.values(options.source_overrides ?? {}).some(rule => !rule.voice_disabled);
  return <section className="vr-overlay-card player-audio-settings" aria-labelledby="player-audio-title">
    <div className="vr-overlay-card-heading vr-overlay-feature-heading"><div><FlaskConical size={18} />
      <span className="player-audio-title-row">
        <strong id="player-audio-title">{t("settings.capture.networkTitle")}</strong>
        <button type="button" className="player-audio-help-button" aria-label={disclaimerTitle} title={disclaimerTitle}
          onClick={() => setDisclaimerOpen(true)}>
          <CircleHelp size={16} aria-hidden="true" />
        </button>
      </span>
    </div>
      <PreferenceToggle title={t("settings.capture.networkEnable")} hideLabel
        checked={options.enabled === true} disabled={disabled && !options.enabled} onChange={(enabled) => onChange({ enabled })} />
    </div>
    {!linux && showUnsupportedNotice && memory?.status?.supported === false && <p className="settings-field-note" role="status">{t("settings.capture.unsupported")}</p>}
    {!linux && memory && memory.status?.supported !== false && <MemoryCompressionControls memory={memory} />}
    <div className="form-grid">
      {!linux && <Select label={t("settings.audio.players.drive")} value={options.pagefile_drive.toUpperCase()}
        values={Array.from({ length: 26 }, (_, i) => `${String.fromCharCode(65 + i)}:`)}
        disabled={fieldsDisabled} onChange={(pagefile_drive) => onChange({ pagefile_drive })} />}
      {!linux && <div className="field">
        <span>{t("settings.audio.players.opus")}</span>
        <EditableDropdownField label={t("settings.audio.players.opus")}
          placeholder={t("settings.audio.players.opusAuto")} value={options.opus_library_path}
          options={[]} disabled={fieldsDisabled} commitOnBlur
          onChange={(path) => onChange({ opus_library_path: path.trim() })} />
      </div>}
      <RangeField label={t("settings.audio.players.limit")} value={options.max_players}
        min={0} max={8} step={1} disabled={fieldsDisabled} formatValue={formatPlayerCount}
        onCommit={(max_players) => onChange({ max_players })} />
      <RangeField label={t("settings.audio.players.visibleLimit")} value={options.max_visible_players ?? 4}
        min={0} max={64} step={1} disabled={fieldsDisabled} formatValue={formatPlayerCount}
        onCommit={(max_visible_players) => onChange({ max_visible_players })} />
    </div>
    <div className={`settings-field-note player-audio-status${phase === "streaming" ? " is-active" : ""}`} role="status">
      <div className="player-audio-status-head">
        <span className="player-audio-status-dot" aria-hidden="true" />
        <strong>{phase === "streaming" && status
          ? noVoiceSources ? t("settings.audio.players.noVoiceSources")
            : status.active_players > 0 ? t("settings.audio.players.activeTracks", { players: status.active_players })
              : t("settings.audio.players.waitingSpeech")
          : t(`settings.audio.players.phase.${phase}`, { defaultValue: t("status.networkStarting") })}</strong>
      </div>
      {phase === "acquiring_key" && keyScan && <p>{t("settings.audio.players.keyElapsed", {
        seconds: (keyScan.elapsed_ms / 1000).toFixed(1),
      })}</p>}
      {phase === "streaming" && keyReady && keyScan && <p>{t(`settings.audio.players.keyStage.${keyScan.stage}`)}
        {keyScan.stage === "verified" && <> · {t("settings.audio.players.keyTime", {
          seconds: (keyScan.elapsed_ms / 1000).toFixed(1),
        })}</>}
      </p>}
      {status?.detail && <p className="settings-validation-error">{status.detail}</p>}
    </div>
    {disclaimerOpen && <PlayerAudioStartDialog readOnly os={os} onDecision={() => setDisclaimerOpen(false)} />}
  </section>;
}
