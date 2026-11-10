import { createContext, useContext, type ReactNode } from "react";
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
  vr: { subtitles: Capability; controller_input: Capability; scene_capture: Capability };
  update_method: "in_app" | "manual";
}

export interface PlatformState {
  core: CoreCapabilities | null;
  desktop: DesktopCapabilities | null;
}

export const PlatformCapabilitiesContext = createContext<PlatformState>({ core: null, desktop: null });
export const usePlatformCapabilities = () => useContext(PlatformCapabilitiesContext);

export function CapabilityNotice({ capability }: { capability: Capability | null | undefined }) {
  const { t } = useTranslation();
  if (capability?.available) return null;
  return <p className="settings-validation-error" role="status">
    {t(capability?.reason_code ? capability.reason_code.startsWith("platform.") ? capability.reason_code
      : capability.reason_code.startsWith("vr.") ? `platform.${capability.reason_code}` : `errors.${capability.reason_code}` : "common.checking")}
  </p>;
}

export function CapabilitySection({ capability, children }: { capability: Capability | null | undefined; children: ReactNode }) {
  if (capability?.supported) return <>{children}</>;
  return <>
    <CapabilityNotice capability={capability} />
    <fieldset disabled={!capability?.supported} className="platform-capability-section">{children}</fieldset>
  </>;
}
