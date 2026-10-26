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
  // The dedicated progress overlay already reports key acquisition and retries.
  if (["waiting_key", "acquiring_key", "retrying_key", "streaming"].includes(phase)) return null;
  return { key: phase, text: [t(`settings.audio.players.phase.${phase}`), status.detail].filter(Boolean).join("\n"), duration_ms: null };
}
