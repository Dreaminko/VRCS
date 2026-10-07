import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { VrOverlayStatus } from "./integrations/types";
import type { VrDashboardAction } from "./vr-dashboard/settings-actions";
import type { VrOverlayPositionField } from "./settings/vr-overlay-settings";

export type VrOverlayKind = "headset" | "wrist";

export const VR_OVERLAY_STATUS_EVENT = "vr-overlay-status-changed";
export const VR_DASHBOARD_ACTION_EVENT = "vr-dashboard-action";

export interface VrDashboardChoice {
  value: string;
  options: Array<{ value: string; label: string }>;
}

export interface VrDashboardNumberField {
  field: VrOverlayPositionField;
  label: string;
  value: string;
  can_decrease: boolean;
  can_increase: boolean;
}

export interface VrDashboardViewModel {
  labels: {
    title: string;
    subtitle: string;
    master: string;
    headset: string;
    wrist: string;
    ocr: string;
    content: string;
    hand: string;
    width: string;
    opacity: string;
    position: string;
    rotation: string;
    reset_position: string;
    gesture: string;
    preview: string;
    bindings: string;
    saving: string;
    saved: string;
    display_tab: string;
    language_tab: string;
    osc_tab: string;
    recognition_language: string;
    translation_mode: string;
    translation_languages: string;
    speaker_language: string;
    microphone_language: string;
    add_target: string;
    presets: string;
    save_preset: string;
    apply_preset: string;
    delete: string;
    osc_original: string;
    osc_enabled: string;
    osc_mute_sync: string;
    osc_mute_toast: string;
    osc_strategy: string;
    osc_hint: string;
    close: string;
  };
  enabled: boolean;
  headset: { enabled: boolean; content: string; width: string; opacity: string; position: VrDashboardNumberField[] };
  wrist: { enabled: boolean; hand: string; content: string; width: string; opacity: string; position: VrDashboardNumberField[] };
  ocr: { enabled: boolean; backend: string; gesture: boolean };
  language: {
    recognition: VrDashboardChoice; mode: VrDashboardChoice;
    speaker_targets: VrDashboardChoice[]; microphone_targets: VrDashboardChoice[];
    can_add: boolean; presets: VrDashboardChoice; can_save_preset: boolean;
  };
  osc: { enabled: boolean; original: boolean; mute_sync: boolean; mute_toast: boolean; strategy: string; endpoint: string };
  status: string;
  save_state: "idle" | "saving" | "saved" | "error";
  error: string | null;
}

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
  dashboard: { state: "disabled", visible: false, last_error_code: null },
  last_error_detail: null,
  ocr: { state:"disabled",scan_id:0,block_count:0,controller_bound:false,gesture_available:false,layout_limited:false,completed_translations:0,failed_translations:0,timed_out:false,last_error_code:null,last_error:null },
};

export async function getVrOverlayStatus(): Promise<VrOverlayStatus> {
  if (!isTauri()) return UNSUPPORTED_VR_OVERLAY_STATUS;
  return invoke<VrOverlayStatus>("vr_overlay_status");
}

export async function retryVrOverlay(): Promise<void> {
  if (!isTauri()) return;
  await invoke("vr_overlay_retry");
}

export async function openVrOcrBindings(): Promise<void> {
  if (!isTauri()) return;
  await invoke("vr_ocr_open_bindings");
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

export async function updateVrDashboardView(view: VrDashboardViewModel): Promise<void> {
  if (!isTauri()) return;
  await invoke("vr_dashboard_update_view", { view });
}

export async function listenVrDashboardActions(
  onAction: (action: VrDashboardAction) => void,
): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<VrDashboardAction>(VR_DASHBOARD_ACTION_EVENT, (event) => {
    onAction(event.payload);
  });
}
