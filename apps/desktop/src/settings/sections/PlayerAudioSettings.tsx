import { FlaskConical } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { PlayerAudioSettings as Options } from "../../capture/types";
import { MemoryCompressionControls } from "../../capture/MemoryCompressionControls";
import type { PlayerAudioStart } from "../../capture/usePlayerAudioStart";
import { usePlatformCapabilities } from "../../core-client/capabilities";
import type { PlayerAudioStatus } from "../../core-client/types";
import { EditableDropdownField } from "../../shared/ui/DropdownField";
import { PreferenceToggle, RangeField, Select } from "../SettingsControls";

export const DEFAULT_PLAYER_AUDIO: Options = {
  enabled: false, pagefile_drive: "C:", opus_library_path: "", max_players: 0, max_visible_players: 4,
  proximity: { enabled: false, radius_m: 10 },
};

export function PlayerAudioSettings({ value, status, memory, disabled, onChange }: {
  value?: Options;
  status?: PlayerAudioStatus | null;
  memory?: PlayerAudioStart["memory"];
  disabled: boolean;
  onChange: (patch: Partial<Options>) => void;
}) {
  const { t } = useTranslation();
  const linux = usePlatformCapabilities().core?.os === "linux";
  const options = value ?? DEFAULT_PLAYER_AUDIO;
  const fieldsDisabled = disabled || !options.enabled;
  const phase = status?.phase ?? "stopped";
  const keyScan = status?.key_scan;
  const keyReady = keyScan?.stage === "verified" || keyScan?.stage === "cached";
  const formatPlayerCount = (count: number) => count === 0 ? t("settings.audio.players.unlimited") : String(count);
  return <section className="vr-overlay-card player-audio-settings" aria-labelledby="player-audio-title">
    <div className="vr-overlay-card-heading vr-overlay-feature-heading"><div><FlaskConical size={18} />
      <span><strong id="player-audio-title">{t("settings.capture.networkTitle")}</strong></span>
    </div>
      <PreferenceToggle title={t("settings.capture.networkEnable")} hideLabel
        checked={options.enabled === true} disabled={disabled && !options.enabled} onChange={(enabled) => onChange({ enabled })} />
    </div>
    {!linux && memory?.status?.supported === false && <p className="settings-field-note" role="status">{t("settings.capture.unsupported")}</p>}
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
    <div className="settings-field-note" role="status">
      <strong>{phase === "streaming" && status
        ? options.selected_tracks?.length === 0 ? t("live.players.noneSelected")
          : status.active_players > 0 ? t("settings.audio.players.activeTracks", { players: status.active_players })
            : t("settings.audio.players.waitingSpeech")
        : t(`settings.audio.players.phase.${phase}`, { defaultValue: t("status.networkStarting") })}</strong>
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
  </section>;
}
