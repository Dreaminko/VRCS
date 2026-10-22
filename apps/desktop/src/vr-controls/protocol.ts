export type ControlAction =
  | { set_wrist_enabled: boolean }
  | { set_headset_enabled: boolean }
  | { set_osc_enabled: boolean }
  | { set_capture_running: boolean };

export type ControlKey = "wrist" | "headset" | "osc" | "capture";
export type ControlSource = "dashboard" | "wrist_menu" | "dev_preview";

export interface ControlRequest {
  owner_epoch: number;
  request_id: number;
  base_revision: number;
  source: ControlSource;
  action: ControlAction;
}

export interface ControlItem {
  requested: boolean;
  effective: boolean;
  available: boolean;
  busy: boolean;
  label: string;
  reason: string | null;
}

export interface ControlSnapshot {
  owner_epoch: number;
  revision: number;
  items: ControlItem[];
  dark: boolean;
  labels: Record<string, string>;
}

export interface PlacementRequest { owner_epoch: number; id: number; mode: "wrist" | "world" }
