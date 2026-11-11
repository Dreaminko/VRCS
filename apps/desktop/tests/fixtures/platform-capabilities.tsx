import type { ReactNode } from "react";
import { PlatformCapabilitiesContext } from "../../src/core-client/capabilities";
import type { Capability, PlatformState } from "../../src/core-client/capabilities";

const available: Capability = { supported: true, available: true, reason_code: null };

export function WindowsCapabilities({ children, networkSupported = true, networkAvailable = networkSupported, credentials = available }: { children: ReactNode; networkSupported?: boolean; networkAvailable?: boolean; credentials?: Capability }) {
  const network: Capability = networkAvailable ? available : {
    supported: networkSupported, available: false,
    reason_code: networkSupported ? "platform.network_helper_missing" : "platform.network_capture_unsupported",
  };
  const value: PlatformState = {
    core: {
      os: "windows", arch: "x86_64",
      audio: { microphone: available, system_output: available, application_output: available, player_audio: network },
      credentials,
      inference: { vad_cpu: available, qwen_cpu: available, qwen_vulkan: available, local_translation: available, ocr_cpu: available, ocr_directml: available },
      chatbox: { osc_send: available, network_receive: network },
    },
    desktop: {
      os: "windows", arch: "x86_64", session_type: "windows",
      screenshot: available, always_on_top: available, global_shortcuts: available,
      vr: { subtitles: available, controller_input: available, scene_capture: available },
      update_method: "in_app",
    },
  };
  return <PlatformCapabilitiesContext.Provider value={value}>{children}</PlatformCapabilitiesContext.Provider>;
}
