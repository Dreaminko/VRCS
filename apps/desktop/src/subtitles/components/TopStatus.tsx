import { useTranslation } from "react-i18next";

import type { ConnectionState, Health, Settings } from "../../types";

export function TopStatus({ connection, health, settings }: {
  connection: ConnectionState;
  health: Health | null;
  settings: Settings | null;
}) {
  const { t } = useTranslation();
  const connectionLabel = t(`status.connection.${connection}`);
  return (
    <div className="top-status-row">
      <div className="status-summary" aria-label={t("status.summary")}>
        <div className={`core-summary connection-${connection}`}><span>Core</span><strong><i aria-hidden="true" />{connectionLabel}</strong></div>
        <i aria-hidden="true" />
        <div><span>{t("status.label")}</span><strong>{transcriptionStatusLabel(health, t)}</strong></div>
        <i aria-hidden="true" />
        <div className={health?.osc?.send_gate === "open" ? "mute-summary" : "mute-summary muted"}>
          <span>{t("status.vrchat")}</span>
          <strong>{vrchatSendStatusLabel(health, settings, t)}</strong>
        </div>
        <i aria-hidden="true" />
        <div><span>{t("status.engine")}</span><strong>{engineLabel(settings)}</strong></div>
      </div>
    </div>
  );
}

function transcriptionStatusLabel(health: Health | null, t: (key: string) => string): string {
  if (!health?.capture_requested) return t("status.waiting");
  if (health.microphone_capture_state === "paused_vrchat_muted") {
    return t("status.microphonePaused");
  }
  return t("status.transcribing");
}

function vrchatSendStatusLabel(
  health: Health | null,
  settings: Settings | null,
  t: (key: string) => string,
): string {
  if (!settings?.osc.enabled || health?.osc?.status === "disabled") return t("status.vrchatSendDisabled");
  if (!health?.osc) return t("status.vrchatSendChecking");
  if (health.osc.status === "error") return t("status.vrchatSendError");
  if (health.osc.send_gate === "blocked_vrchat_muted") return t("status.pausedVrchatMuted");
  if (health.osc.send_gate === "blocked_mute_unknown") return t("status.muteUnknown");
  return t("status.sendReady");
}

function engineLabel(settings: Settings | null): string {
  if (settings?.asr.backend === "qwen_realtime") return "Qwen3 ASR";
  if (settings?.asr.backend === "fun_asr_realtime") return "Fun-ASR";
  if (settings?.asr.backend === "openai_realtime") return "OpenAI Realtime";
  return `Whisper ${capitalize(settings?.asr.local.model ?? "small")}`;
}

function capitalize(value: string): string {
  return value.charAt(0).toUpperCase() + value.slice(1);
}
