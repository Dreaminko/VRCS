import { useEffect, useState } from "react";
import { createLucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { OscInboundRuntimeStatus, OscInboundSettings } from "../../integrations/types";
import { useHealthSelector } from "../../core-client/health-store";
import { PreferenceToggle } from "../SettingsControls";
import { SettingsDisclosure } from "../components/SettingsDisclosure";

const RouteArrowRight = createLucideIcon("route-arrow-right", [
  ["circle", { cx: "6", cy: "5", r: "3", key: "151irh" }],
  ["path", { d: "M9 5h8.5a1 1 0 0 1 0 7h-11a1 1 0 0 0 0 7H21", key: "q947b3" }],
  ["path", { d: "m18 22 3-3-3-3", key: "14hia9" }],
]);

const DEFAULT_INBOUND: OscInboundSettings = {
  enabled: false,
  port: 9002,
  intercept_chatbox_input: true,
  forward_unmatched: true,
};

export function OscInboundSettingsCard({
  config,
  runtime,
  oscEnabled,
  oscPort,
  unitextEnabled,
  unitextPort,
  onChange,
}: {
  config: OscInboundSettings | undefined;
  runtime: OscInboundRuntimeStatus | undefined;
  oscEnabled: boolean;
  oscPort: number;
  unitextEnabled: boolean;
  unitextPort: number;
  onChange: (patch: Partial<OscInboundSettings>) => void;
}) {
  const { t } = useTranslation();
  const liveRuntime = useHealthSelector((health) => health?.osc_inbound) ?? runtime;
  const inbound = config ?? DEFAULT_INBOUND;
  const [portText, setPortText] = useState(String(inbound.port));
  const [portError, setPortError] = useState("");

  useEffect(() => {
    setPortText(String(inbound.port));
    setPortError("");
  }, [inbound.port, oscPort, unitextEnabled, unitextPort]);

  const validatePort = (value: number): string => {
    if (!Number.isInteger(value) || value < 1 || value > 65_535) {
      return t("settings.osc.invalidPort");
    }
    if (value === oscPort) {
      return t("settings.oscInbound.portConflictOsc");
    }
    if (unitextEnabled && (value === unitextPort || value === 9001)) {
      return t("settings.oscInbound.portConflictUniText");
    }
    return "";
  };

  const commitPort = () => {
    const port = Number(portText);
    const error = validatePort(port);
    if (error) {
      setPortError(error);
      return;
    }
    setPortError("");
    if (port !== inbound.port) {
      onChange({ port });
    }
  };

  const state = !inbound.enabled
    ? "disabled"
    : liveRuntime?.state === "error"
      ? "error"
      : "ready";
  const statusKey = !inbound.enabled
    ? "disabled"
    : liveRuntime?.state === "error"
      ? "error"
      : "listening";
  const outputUnavailable = inbound.intercept_chatbox_input && !oscEnabled && !unitextEnabled;
  const statusDetail = liveRuntime?.last_error
    || (outputUnavailable ? t("settings.oscInbound.outputRequiredHint") : null)
    || (liveRuntime && liveRuntime.received_packets > 0
      ? t("settings.oscInbound.stats", {
          intercepted: liveRuntime.intercepted_chatbox_messages,
          forwarded: liveRuntime.forwarded_packets,
        })
      : inbound.forward_unmatched
        ? t("settings.oscInbound.statusHint", { port: oscPort })
        : null);

  return (
    <section className="connections-settings-group osc-section" aria-labelledby="connections-osc-inbound-title">
      <div className="section-heading connections-feature-heading">
        <div>
          <RouteArrowRight size={18} />
          <h3 id="connections-osc-inbound-title">{t("settings.oscInbound.title")}</h3>
        </div>
        <div className="connections-heading-actions">
          <PreferenceToggle
            title={t("settings.oscInbound.enable")}
            hideLabel
            checked={inbound.enabled}
            disabled={false}
            onChange={(enabled) => {
              if (enabled) {
                const error = validatePort(inbound.port);
                if (error) {
                  setPortError(error);
                  return;
                }
              }
              setPortError("");
              onChange({ enabled });
            }}
          />
        </div>
      </div>

      {!inbound.enabled && portError && <small className="error">{portError}</small>}

      <SettingsDisclosure open={inbound.enabled}>
        <div className={`osc-connection ${state}`} aria-live="polite">
          <span className="osc-connection-dot" aria-hidden="true" />
          <div>
            <strong>{t(`settings.oscInbound.status.${statusKey}`)}</strong>
            {statusDetail && <p>{statusDetail}</p>}
          </div>
          <code>{liveRuntime?.listen_address || `127.0.0.1:${inbound.port}`}</code>
        </div>
      </SettingsDisclosure>

      <SettingsDisclosure open={inbound.enabled || Boolean(portError)}>
        <div className="osc-panel enabled">
          <div className="settings-toggle-list settings-feature-toggle">
            <PreferenceToggle
              title={t("settings.oscInbound.interceptChatbox")}
              checked={inbound.intercept_chatbox_input}
              disabled={false}
              onChange={(intercept_chatbox_input) => onChange({ intercept_chatbox_input })}
            />
            <PreferenceToggle
              title={t("settings.oscInbound.forwardUnmatched")}
              checked={inbound.forward_unmatched}
              disabled={false}
              onChange={(forward_unmatched) => onChange({ forward_unmatched })}
            />
          </div>

          <div className="osc-endpoint-row">
            <div className="osc-endpoint-field">
              <span>{t("settings.osc.address")}</span>
              <code className="osc-address-value">127.0.0.1</code>
            </div>
            <label className="osc-endpoint-field osc-port-field">
              <span>{t("settings.oscInbound.listenPort")}</span>
              <input
                type="text"
                inputMode="numeric"
                value={portText}
                aria-invalid={Boolean(portError)}
                aria-describedby={portError ? "osc-inbound-port-help" : undefined}
                onChange={(event) => {
                  setPortText(event.target.value.replace(/\D/g, "").slice(0, 5));
                  setPortError("");
                }}
                onBlur={commitPort}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    event.currentTarget.blur();
                  }
                }}
              />
              {portError && <small id="osc-inbound-port-help" className="error">{portError}</small>}
            </label>
          </div>
        </div>
      </SettingsDisclosure>
    </section>
  );
}
