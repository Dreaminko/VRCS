import { useState, type MouseEvent, type ReactNode } from "react";
import { Activity } from "lucide-react";
import { useTranslation } from "react-i18next";

import { vrchatNetworkStatus } from "../../capture/vrchat-network-status";
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
  const networkStatus = vrchatNetworkStatus(health, settings, t);
  const statusLabel = networkStatus?.label ?? transcriptionStatusLabel(health, t);
  const vrchatStatus = vrchatSendStatus(health, settings, t);
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
          <span className={`status-summary-item status-main${networkStatus ? ` network-status ${networkStatus.tone}` : ""}`}>
            <span className="status-summary-label">{t("status.label")}</span>
            <strong>{statusLabel}</strong>
          </span>
          <i aria-hidden="true" />
          <span className={`status-summary-item mute-summary${vrchatStatus.tone ? ` ${vrchatStatus.tone}` : ""}`}>
            <span className="status-summary-label">{t("status.vrchat")}</span>
            <strong>{vrchatStatus.label}</strong>
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

function vrchatSendStatus(
  health: Health | null,
  settings: Settings | null,
  t: (key: string) => string,
): { label: string; tone: "" | "muted" | "error" } {
  const oscEnabled = Boolean(settings?.osc?.enabled);
  const unitextEnabled = Boolean(settings?.unitext?.enabled);
  if (!oscEnabled && !unitextEnabled) {
    return { label: t("status.vrchatSendDisabled"), tone: "muted" };
  }

  const osc = health?.osc;
  const unitext = health?.unitext;

  if (!oscEnabled) {
    if (!unitext) return { label: t("status.vrchatSendChecking"), tone: "muted" };
    if (unitext.status === "disabled") return { label: t("status.vrchatSendDisabled"), tone: "muted" };
    if (unitext.status === "error") return { label: t("status.vrchatSendError"), tone: "error" };
    if (unitext.status === "ready") return { label: t("status.sendReady"), tone: "" };
    if (unitext.status === "discovering") {
      return { label: t("status.vrchatSendDiscovering"), tone: "muted" };
    }
    if (unitext.status === "not_detected") {
      return { label: t("status.vrchatSendNotDetected"), tone: "muted" };
    }
    return { label: t("status.vrchatSendChecking"), tone: "muted" };
  }

  if (osc?.status === "error" || (unitextEnabled && unitext?.status === "error")) {
    return { label: t("status.vrchatSendError"), tone: "error" };
  }
  if (!osc) {
    if (unitextEnabled && unitext?.status === "ready") return { label: t("status.sendReady"), tone: "" };
    return { label: t("status.vrchatSendChecking"), tone: "muted" };
  }
  if (osc.status !== "disabled" && osc.send_gate === "open") {
    return { label: t("status.sendReady"), tone: "" };
  }
  if (unitextEnabled && unitext?.status === "ready") {
    return { label: t("status.sendReady"), tone: "" };
  }
  if (osc.status === "disabled") return { label: t("status.vrchatSendDisabled"), tone: "muted" };
  if (osc.send_gate === "blocked_vrchat_muted") return { label: t("status.pausedVrchatMuted"), tone: "muted" };
  if (osc.send_gate === "blocked_mute_unknown") return { label: t("status.muteUnknown"), tone: "muted" };
  return { label: t("status.sendReady"), tone: "" };
}

function engineLabel(
  settings: Settings | null,
  apiProfiles: ApiProfileView[],
  providerDefinitions: ProviderDefinition[],
): string {
  return settings
    ? recognitionEngineLabel(settings.asr, apiProfiles, providerDefinitions)
    : "Qwen3-ASR";
}
