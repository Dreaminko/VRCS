import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { VrOverlayStatus } from "./types";

export type VrOverlayKind = "headset" | "wrist";

export const VR_OVERLAY_STATUS_EVENT = "vr-overlay-status-changed";

export const UNSUPPORTED_VR_OVERLAY_STATUS: VrOverlayStatus = {
  state: "unsupported",
  runtime_installed: false,
  hmd_present: false,
  last_connected_at: null,
  reconnect_attempt: 0,
  headset: {
    state: "disabled",
    sample_visible: false,
    last_error_code: null,
  },
  wrist: {
    state: "disabled",
    sample_visible: false,
    bound_role: null,
    tracked_device_available: false,
    last_error_code: null,
  },
  last_error_detail: null,
};

export async function getVrOverlayStatus(): Promise<VrOverlayStatus> {
  if (!isTauri()) return UNSUPPORTED_VR_OVERLAY_STATUS;
  return invoke<VrOverlayStatus>("vr_overlay_status");
}

export async function retryVrOverlay(): Promise<void> {
  if (!isTauri()) return;
  await invoke("vr_overlay_retry");
}

export async function showVrOverlaySample(kind: VrOverlayKind): Promise<void> {
  if (!isTauri()) return;
  await invoke("vr_overlay_show_sample", { kind });
}

export async function hideVrOverlaySample(kind: VrOverlayKind): Promise<void> {
  if (!isTauri()) return;
  await invoke("vr_overlay_hide_sample", { kind });
}

export async function listenVrOverlayStatus(
  onStatus: (status: VrOverlayStatus) => void,
): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<VrOverlayStatus>(VR_OVERLAY_STATUS_EVENT, (event) => {
    onStatus(event.payload);
  });
}
