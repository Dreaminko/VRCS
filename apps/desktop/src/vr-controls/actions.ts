import type { Settings } from "../settings/types";
import type { ControlAction } from "./protocol";

type SettingsAction = ControlAction | { set_anchor_mode: "wrist" | "world" };

export function applyControlAction(settings: Settings, action: SettingsAction): Settings {
  if ("set_capture_running" in action) return settings;
  if ("set_anchor_mode" in action) return settings.vr_overlay.wrist.anchor_mode === action.set_anchor_mode ? settings : {
    ...settings, vr_overlay: { ...settings.vr_overlay, wrist: { ...settings.vr_overlay.wrist, anchor_mode: action.set_anchor_mode } },
  };
  if ("set_osc_enabled" in action) {
    if (!settings.features.osc_chatbox) throw new Error("OSC is unavailable");
    return settings.osc.enabled === action.set_osc_enabled ? settings : {
      ...settings, osc: { ...settings.osc, enabled: action.set_osc_enabled },
    };
  }
  const kind = "set_wrist_enabled" in action ? "wrist" : "headset";
  const enabled = "set_wrist_enabled" in action ? action.set_wrist_enabled : action.set_headset_enabled;
  return settings.vr_overlay[kind].enabled === enabled ? settings : {
    ...settings, vr_overlay: {
      ...settings.vr_overlay, [kind]: { ...settings.vr_overlay[kind], enabled },
    },
  };
}

export function createControlExecutor(options: {
  update: (apply: (settings: Settings) => Settings) => Promise<Settings>;
  setCaptureRunning: (target: boolean) => Promise<boolean>;
}) {
  return (action: SettingsAction): Promise<void> => {
    if ("set_capture_running" in action) return options.setCaptureRunning(action.set_capture_running).then((ok) => {
      if (!ok) throw new Error("Capture action failed");
    });
    return options.update((current) => applyControlAction(current, action)).then((saved) => {
      if (applyControlAction(saved, action) !== saved) throw new Error("Settings action was not applied");
    });
  };
}
