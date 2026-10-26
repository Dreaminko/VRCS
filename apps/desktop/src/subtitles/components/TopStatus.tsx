import { useState, type MouseEvent, type ReactNode } from "react";
import { Activity } from "lucide-react";
import { useTranslation } from "react-i18next";

import { recognitionEngineLabel } from "../../providers/recognition-services";
import type {
  ConnectionState,
  Health,
} from "../../core-client/types";
import type {
  ApiProfileView,
  ProviderDefinition,
} from "../../providers/types";
import type { Settings } from "../../settings/types";
import { useTopStatusMotion } from "./useTopStatusMotion";

const STATUS_EXPANDED_KEY = "vrcs.status-expanded";

export function TopStatus({ connection, health, settings, apiProfiles = [], providerDefinitions = [], controls }: {
  connection: ConnectionState;
  health: Health | null;
  settings: Settings | null;
  apiProfiles?: ApiProfileView[];
  providerDefinitions?: ProviderDefinition[];
  controls?: ReactNode;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(() => {
    try { return localStorage.getItem(STATUS_EXPANDED_KEY) !== "false"; }
    catch { return true; }
  });
  const motion = useTopStatusMotion(expanded);
  const connectionLabel = t(`status.connection.${connection}`);
  const toggleLabel = t(expanded ? "common.collapseNamed" : "common.expandNamed", { name: t("status.summary") });
  const toggle = (event: MouseEvent<HTMLButtonElement>) => {
    motion.prepare(event.detail > 0);
    const next = !expanded;
    setExpanded(next);
    try { localStorage.setItem(STATUS_EXPANDED_KEY, String(next)); }
    catch { /* Keep the control usable when storage is unavailable. */ }
  };
  return (
    <div className="top-status-row" ref={motion.rowRef}>
      {controls}
      <button type="button" ref={motion.buttonRef} className={`status-toggle ${expanded ? "status-summary" : "status-trigger"}`}
        aria-label={toggleLabel} aria-expanded={expanded} data-tooltip={toggleLabel} onClick={toggle}>
        <span className="status-motion-surface" ref={motion.surfaceRef} aria-hidden="true">
          <span className="status-motion-outline" ref={motion.outlineRef} />
          <span className="status-motion-fill" ref={motion.fillRef} />
        </span>
        <span className="status-motion-content" ref={motion.contentRef}>
        {expanded ? <>
          <span className={`status-summary-item core-summary connection-${connection}`}><span className="status-summary-label">Core</span><strong><i aria-hidden="true" />{connectionLabel}</strong></span>
          <i aria-hidden="true" />
          <span className="status-summary-item"><span className="status-summary-label">{t("status.label")}</span><strong>{transcriptionStatusLabel(health, t)}</strong></span>
          <i aria-hidden="true" />
          <span className={`status-summary-item mute-summary${health?.osc?.send_gate === "open" ? "" : " muted"}`}>
            <span className="status-summary-label">{t("status.vrchat")}</span>
            <strong>{vrchatSendStatusLabel(health, settings, t)}</strong>
          </span>
          <i aria-hidden="true" />
          <span className="status-summary-item"><span className="status-summary-label">{t("status.engine")}</span><strong>{engineLabel(settings, apiProfiles, providerDefinitions)}</strong></span>
        </> : <Activity size={18} aria-hidden="true" />}
        </span>
      </button>
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

function engineLabel(
  settings: Settings | null,
  apiProfiles: ApiProfileView[],
  providerDefinitions: ProviderDefinition[],
): string {
  return settings
    ? recognitionEngineLabel(settings.asr, apiProfiles, providerDefinitions)
    : "Qwen3-ASR 0.6B";
}
