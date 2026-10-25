import { FlaskConical } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { PlayerAudioSettings as Options } from "../../capture/types";
import type { PlayerAudioStatus } from "../../core-client/types";
import { EditableDropdownField } from "../../shared/ui/DropdownField";
import { Select } from "../SettingsControls";

export const DEFAULT_PLAYER_AUDIO: Options = {
  pagefile_drive: "C:", opus_library_path: "", max_players: 0, max_visible_players: 4,
};

export function PlayerAudioSettings({ value, status, disabled, onChange }: {
  value?: Options;
  status?: PlayerAudioStatus | null;
  disabled: boolean;
  onChange: (patch: Partial<Options>) => void;
}) {
  const { t } = useTranslation();
  const options = value ?? DEFAULT_PLAYER_AUDIO;
  const phase = status?.phase ?? "stopped";
  const keyScan = status?.key_scan;
  const keyReady = keyScan?.stage === "verified" || keyScan?.stage === "cached";
  return <section className="vr-overlay-card player-audio-settings" aria-labelledby="player-audio-title">
    <div className="vr-overlay-card-heading"><div><FlaskConical size={18} />
      <span><strong id="player-audio-title">{t("settings.audio.players.title")}</strong>
        <small>{t("settings.audio.players.description")}</small></span>
    </div></div>
    <p className="settings-field-note">{t("settings.audio.players.requirements")}</p>
    <div className="vr-overlay-field-grid">
      <Select label={t("settings.audio.players.drive")} value={options.pagefile_drive.toUpperCase()}
        values={Array.from({ length: 26 }, (_, i) => `${String.fromCharCode(65 + i)}:`)}
        disabled={disabled} onChange={(pagefile_drive) => onChange({ pagefile_drive })} />
      <Select label={t("settings.audio.players.limit")} value={String(options.max_players)}
        options={[{value: "0", label: t("settings.audio.players.unlimited")},
          ...["1", "2", "3", "4", "5", "6", "7", "8"].map(value => ({value, label: value}))]} disabled={disabled}
        helper={t("settings.audio.players.limitHint")}
        onChange={(count) => onChange({ max_players: Number(count) })} />
      <Select label={t("settings.audio.players.visibleLimit")} value={String(options.max_visible_players ?? 4)}
        options={[{ value: "0", label: t("settings.audio.players.unlimited") },
          ...Array.from({ length: 64 }, (_, index) => ({ value: String(index + 1), label: String(index + 1) }))]}
        disabled={disabled} helper={t("settings.audio.players.visibleLimitHint")}
        onChange={(count) => onChange({ max_visible_players: Number(count) })} />
    </div>
    <div className="field">
      <span>{t("settings.audio.players.opus")}</span>
      <EditableDropdownField label={t("settings.audio.players.opus")}
        placeholder={t("settings.audio.players.opusAuto")} value={options.opus_library_path}
        options={[]} disabled={disabled} commitOnBlur
        onChange={(path) => onChange({ opus_library_path: path.trim() })} />
    </div>
    <div className="settings-field-note" role="status">
      <strong>{phase === "streaming" && status
        ? options.selected_tracks?.length === 0 ? t("live.players.noneSelected")
          : status.active_players > 0 ? t("settings.audio.players.activeTracks", { players: status.active_players })
            : t("settings.audio.players.waitingSpeech")
        : t(`settings.audio.players.phase.${phase}`)}</strong>
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
