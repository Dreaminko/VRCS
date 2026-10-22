import type { VrDashboardChoice, VrDashboardViewModel } from "../integrations/vr-overlay-native";
import type { VrDashboardAction } from "../vr-dashboard/settings-actions";

export type DashboardPage = "display" | "language" | "osc";
export type TargetGroup = "speaker" | "microphone";
export type DisplayKind = "headset" | "wrist";
export type DashboardAction = VrDashboardAction | "recenter_dialogue" | "attach_dialogue";
export type DashboardPicker =
  | { type: "recognition" | "mode" | "apply_preset" | "delete_preset" }
  | { type: "target"; value: { group: TargetGroup; index: number } };

export interface DashboardUi {
  page: DashboardPage;
  picker: DashboardPicker | null;
  picker_page: number;
  display_editor: DisplayKind | null;
  confirmation: { action: DashboardAction; label: string } | null;
}

export type DashboardCommand =
  | { type: "page"; value: DashboardPage }
  | { type: "picker"; value: DashboardPicker }
  | { type: "picker_page"; value: -1 | 1 }
  | { type: "select"; value: number }
  | { type: "position"; value: DisplayKind }
  | { type: "close" | "confirm" | "cancel" }
  | { type: "action"; value: DashboardAction };

export const DASHBOARD_PICKER_PAGE_SIZE = 32;

export function dashboardPickerChoice(view: VrDashboardViewModel, picker: DashboardPicker): VrDashboardChoice | undefined {
  switch (picker.type) {
    case "recognition": return view.language.recognition;
    case "mode": return view.language.mode;
    case "target": return view.language[`${picker.value.group}_targets`][picker.value.index];
    case "apply_preset":
    case "delete_preset": return view.language.presets;
  }
}
