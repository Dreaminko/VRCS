export type ControlAction =
  | { set_wrist_enabled: boolean }
  | { set_headset_enabled: boolean }
  | { set_osc_enabled: boolean }
  | { set_capture_running: boolean }
  | { set_ocr_gesture_enabled: boolean }
  | { set_network_capture_running: boolean };

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
  hidden?: boolean;
}

export interface ControlSnapshot {
  owner_epoch: number;
  revision: number;
  items: ControlItem[];
  dark: boolean;
  labels: Record<string, string>;
}

export interface PlacementRequest { owner_epoch: number; id: number; mode: "wrist" | "world"; follow_view: boolean }

export interface LayoutEditCommitRequest {
  owner_epoch: number;
  id: number;
  headset?: {
    offset_x_m: number;
    offset_y_m: number;
    distance_m: number;
    pitch_deg: number;
    yaw_deg: number;
    roll_deg: number;
    width_m: number;
  };
  wrist?: {
    offset_x_m: number;
    offset_y_m: number;
    offset_z_m: number;
    pitch_deg: number;
    yaw_deg: number;
    roll_deg: number;
    width_m: number;
  };
  wrist_menu?: {
    offset_x_m: number;
    offset_y_m: number;
    offset_z_m: number;
    pitch_deg: number;
    yaw_deg: number;
    roll_deg: number;
    width_m: number;
  };
}
