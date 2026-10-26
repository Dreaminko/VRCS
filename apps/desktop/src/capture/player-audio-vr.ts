import type { TFunction } from "i18next";
import type { PlayerAudioStatus } from "../core-client/types";
import type { ProgressView } from "../vr-surface/protocol";

export const PLAYER_AUDIO_READING_MS = 3_000;
export interface PlayerAudioPreparation {
  stage: "privileges" | "memory" | "relaunching" | "blocked" | "failed";
  detail: string | null;
}
export interface PlayerAudioPrompt {
  request_id: number;
  title: string;
  intro: string;
  sections: { title: string; paragraphs: string[] }[];
  remaining_seconds: number;
  reading_status: string;
  confirm: string;
  cancel: string;
  previous: string;
  next: string;
  scroll_hint: string;
}
export interface PlayerAudioVrView { notice: PlayerAudioPrompt | null; progress: ProgressView | null }
export interface PlayerAudioDecision { owner_epoch: number; request_id: number; accepted: boolean }

export function playerAudioPrompt(notice: { requestId: number; openedAt: number } | null, now: number, t: TFunction): PlayerAudioPrompt | null {
  if (!notice) return null;
  const remaining = Math.max(0, Math.ceil((PLAYER_AUDIO_READING_MS - (now - notice.openedAt)) / 1_000));
  return {
    request_id: notice.requestId,
    title: t("warnings.playerAudio.title"), intro: t("warnings.playerAudio.intro"),
    sections: ["principle", "terms", "anticheat", "privacy", "disclaimer"].map(key => ({
      title: t(`warnings.playerAudio.${key}Title`),
      paragraphs: [t(`warnings.playerAudio.${key}`),
        ...(key === "principle" ? [t("warnings.playerAudio.paging"), t("settings.audio.players.nameHint")] : []),
        ...(key === "privacy" ? [t("warnings.playerAudio.privacyConsent")] : [])],
    })),
    remaining_seconds: remaining,
    reading_status: remaining ? t("warnings.playerAudio.countdown", { seconds: remaining }) : t("warnings.playerAudio.ready"),
    confirm: t("warnings.playerAudio.confirm"), cancel: t("common.cancel"),
    previous: t("wristReader.previousPage"), next: t("wristReader.nextPage"),
    scroll_hint: t("warnings.playerAudio.vrPreparation.scrollHint"),
  };
}

export function playerAudioProgress(preparation: PlayerAudioPreparation | null, status: PlayerAudioStatus | null | undefined, requested: boolean, t: TFunction): ProgressView | null {
  if (!preparation && (!requested || !status || status.phase === "stopped" || status.phase === "streaming")) return null;
  const stages: ProgressView["stages"] = ["pending", "pending", "pending", "pending"];
  let label: string;
  let detail = "";
  let fraction: [number, number] | null = null;
  if (preparation) {
    const blocked = preparation.stage === "blocked" || preparation.stage === "failed";
    stages[0] = blocked ? "failed" : "active";
    label = t(`warnings.playerAudio.vrPreparation.${preparation.stage}`);
    detail = [preparation.detail, t("warnings.playerAudio.vrPreparation.desktop")].filter(Boolean).join(" · ");
  } else {
    const phase = status!.phase;
    stages[0] = "complete";
    const waitingGame = phase === "waiting_game" || phase === "waiting_world";
    stages[1] = waitingGame ? "active" : "complete";
    if (!waitingGame) stages[2] = phase === "failed" ? "failed" : "active";
    label = t(`settings.audio.players.phase.${phase}`);
    detail = status!.detail ?? "";
    const scan = status!.key_scan;
    if (phase === "acquiring_key" && scan) {
      label = t(`warnings.playerAudio.vrPreparation.${scan.stage === "paging" ? "paging" : "scanning"}`);
      detail = t("settings.audio.players.keyElapsed", { seconds: (scan.elapsed_ms / 1000).toFixed(1) });
      if (scan.stage === "scanning" && scan.total_bytes > 0) fraction = [Math.min(scan.scanned_bytes, scan.total_bytes), scan.total_bytes];
    }
  }
  return {
    scan_id: 0, status: label, detail, elapsed_seconds: Math.floor((status?.key_scan?.elapsed_ms ?? 0) / 1_000),
    stages, stage_labels: ["prerequisites", "game", "key", "audio"].map(key => t(`warnings.playerAudio.vrPreparation.${key}`)),
    translation_fraction: fraction,
  };
}
