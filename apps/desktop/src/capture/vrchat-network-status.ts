import type { TFunction } from "i18next";

import type { Health } from "../core-client/types";
import type { Settings } from "../settings/types";
import { usesVrchatNetwork } from "./vrchat-network.ts";

export type VrchatNetworkTone = "active" | "waiting" | "idle" | "error";

export interface VrchatNetworkStatus {
  label: string;
  tone: VrchatNetworkTone;
}

export function vrchatNetworkStatus(
  health: Health | null,
  settings: Settings | null,
  t: TFunction,
): VrchatNetworkStatus | null {
  if (!usesVrchatNetwork(settings)) return null;
  if (!health) return { label: t("status.networkChecking"), tone: "waiting" };
  if (!health.capture_requested) return { label: t("status.networkStopped"), tone: "idle" };

  const voice = health.player_audio;
  const chatbox = health.vrchat_chatbox;
  const status = voice ?? chatbox;
  if (!status) return { label: t("status.networkStarting"), tone: "waiting" };

  switch (status.phase) {
    case "authorizing":
      return { label: t("status.networkAuthorizing"), tone: "waiting" };
    case "waiting_game":
      return { label: t("status.networkWaitingGame"), tone: "waiting" };
    case "waiting_world":
      return { label: t("status.networkWaitingWorld"), tone: "waiting" };
    case "waiting_traffic":
      return { label: t("status.networkWaitingTraffic"), tone: "waiting" };
    case "waiting_key":
      return { label: t("status.networkVerifyingKey"), tone: "waiting" };
    case "acquiring_key":
      return { label: t("status.networkAcquiringKey"), tone: "waiting" };
    case "retrying_key":
      return { label: t("status.networkRetryingKey"), tone: "waiting" };
    case "streaming":
      if (chatbox && !voice) return { label: t("status.networkReceivingChatbox"), tone: "active" };
      return voice && voice.active_players > 0
        ? { label: t("status.networkReceivingPlayers", { players: voice.active_players }), tone: "active" }
        : { label: t("status.networkWaitingPlayers"), tone: "active" };
    case "failed":
      return { label: t("status.networkFailed"), tone: "error" };
    case "stopped":
      return { label: t("status.networkStopped"), tone: "idle" };
    default:
      return { label: t("status.networkStarting"), tone: "waiting" };
  }
}
