import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import type { VrOverlayStatus } from "../integrations/types";
import type { Settings } from "../settings/types";
import {
  getVrOverlayStatus,
  hideVrOverlaySample,
  listenVrDashboardActions,
  listenVrOverlayStatus,
  openVrOcrBindings,
  showVrOverlaySample,
  updateVrDashboardView,
  type VrDashboardViewModel,
} from "../vr-overlay-native";
import { applyVrDashboardAction, type VrDashboardAction } from "./settings-actions";

type SaveState = VrDashboardViewModel["save_state"];

export function useVrDashboardBridge(
  settings: Settings | null,
  save: (settings: Settings) => Promise<Settings>,
): void {
  const { t } = useTranslation();
  const [status, setStatus] = useState<VrOverlayStatus | null>(null);
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const [saveError, setSaveError] = useState<string | null>(null);
  const settingsRef = useRef(settings);
  const statusRef = useRef(status);
  const saveRef = useRef(save);
  const saveVersionRef = useRef(0);

  settingsRef.current = settings;
  statusRef.current = status;
  saveRef.current = save;

  useEffect(() => {
    let disposed = false;
    let unlisten: () => void = () => undefined;
    void getVrOverlayStatus().then((next) => {
      if (!disposed) setStatus(next);
    });
    void listenVrOverlayStatus((next) => setStatus(next)).then((next) => {
      if (disposed) next();
      else unlisten = next;
    });
    return () => {
      disposed = true;
      unlisten();
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: () => void = () => undefined;
    const handleAction = async (action: VrDashboardAction) => {
      if (action === "preview_headset" || action === "preview_wrist") {
        const kind = action === "preview_headset" ? "headset" : "wrist";
        const visible = statusRef.current?.[kind].sample_visible ?? false;
        await (visible ? hideVrOverlaySample(kind) : showVrOverlaySample(kind));
        return;
      }
      if (action === "open_ocr_bindings") {
        await openVrOcrBindings();
        return;
      }

      const current = settingsRef.current;
      if (!current) return;
      const next = applyVrDashboardAction(current, action);
      settingsRef.current = next;
      const version = ++saveVersionRef.current;
      setSaveState("saving");
      setSaveError(null);
      try {
        const saved = await saveRef.current(next);
        if (version !== saveVersionRef.current) return;
        settingsRef.current = saved;
        setSaveState("saved");
      } catch {
        if (version !== saveVersionRef.current) return;
        setSaveState("error");
        setSaveError(t("settings.vrDashboard.saveFailed"));
      }
    };
    void listenVrDashboardActions((action) => {
      void handleAction(action).catch(() => {
        setSaveState("error");
        setSaveError(t("settings.vrDashboard.actionFailed"));
      });
    }).then((next) => {
      if (disposed) next();
      else unlisten = next;
    });
    return () => {
      disposed = true;
      unlisten();
    };
  }, [t]);

  useEffect(() => {
    if (!settings) return;
    const overlay = settings.vr_overlay;
    const model: VrDashboardViewModel = {
      labels: {
        title: t("settings.vrDashboard.title"),
        subtitle: t("settings.vrDashboard.subtitle"),
        master: t("settings.vrDashboard.master"),
        headset: t("settings.vrDashboard.headset"),
        wrist: t("settings.vrDashboard.wrist"),
        ocr: t("settings.vrDashboard.ocr"),
        content: t("settings.vrOverlay.translationDisplay"),
        hand: t("settings.vrDashboard.hand"),
        width: t("settings.vrDashboard.width"),
        opacity: t("settings.vrDashboard.opacity"),
        gesture: t("settings.vrDashboard.gesture"),
        preview: t("settings.vrDashboard.preview"),
        bindings: t("settings.vrDashboard.bindings"),
        saving: t("settings.vrDashboard.saving"),
        saved: t("settings.vrDashboard.saved"),
      },
      enabled: overlay.enabled,
      headset: {
        enabled: overlay.headset.enabled,
        content: t(overlay.translation_display === "all_languages"
          ? "settings.vrOverlay.translationDisplays.allLanguages"
          : "settings.vrOverlay.translationDisplays.preferredOnly"),
        width: `${overlay.headset.width_m.toFixed(1)} m`,
        opacity: `${Math.round(overlay.headset.opacity * 100)}%`,
      },
      wrist: {
        enabled: overlay.wrist.enabled,
        hand: t(`settings.vrOverlay.hands.${overlay.wrist.hand}`),
        content: t(overlay.translation_display === "all_languages"
          ? "settings.vrOverlay.translationDisplays.allLanguages"
          : "settings.vrOverlay.translationDisplays.preferredOnly"),
        width: `${overlay.wrist.width_m.toFixed(2)} m`,
        opacity: `${Math.round(overlay.wrist.opacity * 100)}%`,
      },
      ocr: {
        enabled: overlay.ocr.enabled,
        backend: t(`settings.vrOcr.backends.${overlay.ocr.backend}`),
        gesture: overlay.ocr.hand_gesture_enabled,
      },
      status: status
        ? t(`settings.vrOverlay.runtimeStates.${status.state}`)
        : t("settings.vrDashboard.connecting"),
      save_state: saveState,
      error: saveError,
    };
    void updateVrDashboardView(model);
  }, [saveError, saveState, settings, status, t]);
}
