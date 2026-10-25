import type { TFunction } from "i18next";
import type { PlayerAudioStatus } from "../core-client/types";

export interface PlayerAudioNotice {
  key: string;
  text: string;
  duration_ms: number | null;
}

// Share the desktop wording with the VR surfaces. Notices are transient UI state,
// never subtitles, translation jobs, or saved conversation entries.
export function playerAudioNotice(status: PlayerAudioStatus | null | undefined, running: boolean, t: TFunction): PlayerAudioNotice | null {
  if (!running || !status || status.phase === "stopped") return null;
  const phase = status.phase;
  if (phase === "streaming") {
    const stage = status.key_scan?.stage;
    if (stage !== "verified" && stage !== "cached") return null;
    const time = stage === "verified" ? ` · ${t("settings.audio.players.keyTime", { seconds: (status.key_scan!.elapsed_ms / 1000).toFixed(1) })}` : "";
    return { key: `ready:${stage}:${status.key_scan!.elapsed_ms}`, text: t(`settings.audio.players.keyStage.${stage}`) + time, duration_ms: 5_000 };
  }
  const detail = phase === "acquiring_key" && status.key_scan
    ? t("settings.audio.players.keyElapsed", { seconds: (status.key_scan.elapsed_ms / 1000).toFixed(1) })
    : status.detail;
  return { key: phase, text: [t(`settings.audio.players.phase.${phase}`), detail].filter(Boolean).join("\n"), duration_ms: null };
}
