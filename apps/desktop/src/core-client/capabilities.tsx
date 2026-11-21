import { createContext, useContext, type ReactNode } from "react";
import { Info, TriangleAlert } from "lucide-react";
import { useTranslation } from "react-i18next";

export interface Capability {
  supported: boolean;
  available: boolean;
  reason_code: string | null;
}

export interface CoreCapabilities {
  os: string;
  arch: string;
  audio: { microphone: Capability; system_output: Capability; application_output: Capability; player_audio: Capability };
  credentials: Capability;
  inference: { vad_cpu: Capability; qwen_cpu: Capability; qwen_vulkan: Capability; local_translation: Capability; ocr_cpu: Capability; ocr_directml: Capability };
  chatbox: { osc_send: Capability; network_receive: Capability };
}

export interface DesktopCapabilities {
  os: string;
  arch: string;
  session_type: string;
  screenshot: Capability;
  always_on_top: Capability;
  global_shortcuts: Capability;
  shortcut_bindings: string[] | null;
  vr: { subtitles: Capability; controller_input: Capability; scene_capture: Capability };
}

export interface PlatformState {
  core: CoreCapabilities | null;
  desktop: DesktopCapabilities | null;
}

export const PlatformCapabilitiesContext = createContext<PlatformState>({ core: null, desktop: null });
export const usePlatformCapabilities = () => useContext(PlatformCapabilitiesContext);

export function CapabilityNotice({
  capability,
  tone,
}: {
  capability: Capability | null | undefined;
  tone?: "info" | "warning" | "error";
}) {
  const { t } = useTranslation();
  if (!capability || capability.available || !capability.reason_code) return null;
  const resolvedTone = tone ?? (capability.supported ? "warning" : "info");
  const Icon = resolvedTone === "info" ? Info : TriangleAlert;
  const key = capability.reason_code.startsWith("platform.")
    ? capability.reason_code
    : capability.reason_code.startsWith("vr.")
      ? `platform.${capability.reason_code}`
      : `errors.${capability.reason_code}`;
  return (
    <p className={`capability-notice is-${resolvedTone}`} role="status">
      <Icon size={15} aria-hidden="true" />
      <span>{t(key)}</span>
    </p>
  );
}

export function CapabilitySection({ capability, children }: { capability: Capability | null | undefined; children: ReactNode }) {
  if (!capability) return <>{children}</>;
  if (capability.supported) {
    return <>
      {!capability.available && <CapabilityNotice capability={capability} />}
      {children}
    </>;
  }
  return <>
    <CapabilityNotice capability={capability} />
    <fieldset disabled className="platform-capability-section">{children}</fieldset>
  </>;
}
