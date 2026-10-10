import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import type { VrOverlayStatus } from "../integrations/types";
import type { Settings } from "../settings/types";
import type { ApiProfileView } from "../providers/types";
import { liveTranslationServiceName } from "../recognition-services";
import { localizedLanguageName } from "../translation-languages";
import { supportsTranslation } from "../api-profile-purpose";
import { VR_OVERLAY_POSITION_RANGES, type VrOverlayDisplayKind, type VrOverlayPositionField } from "../settings/vr-overlay-settings";
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
import { applyVrDashboardAction, dashboardLanguageChoices, type VrDashboardAction } from "./settings-actions";

type SaveState = VrDashboardViewModel["save_state"];

export function useVrDashboardBridge(
  settings: Settings | null,
  save: (settings: Settings) => Promise<Settings>,
  profiles: ApiProfileView[],
): void {
  const { t, i18n } = useTranslation();
  const [status, setStatus] = useState<VrOverlayStatus | null>(null);
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const [saveError, setSaveError] = useState<string | null>(null);
  const settingsRef = useRef(settings);
  const statusRef = useRef(status);
  const saveRef = useRef(save);
  const saveVersionRef = useRef(0);
  const profilesRef = useRef(profiles);

  settingsRef.current = settings;
  statusRef.current = status;
  saveRef.current = save;
  profilesRef.current = profiles;

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
      const current = settingsRef.current;
      if (!current?.features.vr_overlay) return;
      if (action === "open_ocr_bindings" && !current.features.ocr) return;
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


      if (typeof action === "object" && "save_language_preset" in action) {
        action = { save_language_preset: t("settings.translation.presetDefaultName", { count: current.language_presets.length + 1 }) };
      }
      const next = applyVrDashboardAction(current, action, profilesRef.current);
      if (next === current) return;
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
    if (!settings?.features.vr_overlay) return;
    const overlay = settings.vr_overlay;
    const positionFields = (kind: VrOverlayDisplayKind) => Object.entries(VR_OVERLAY_POSITION_RANGES[kind]).map(([name, range]) => {
      const field = name as VrOverlayPositionField;
      const value = (overlay[kind] as unknown as Record<VrOverlayPositionField, number>)[field];
      const labelKeys = { offset_x_m: "horizontal", offset_y_m: "vertical", offset_z_m: "depth", distance_m: "distance", pitch_deg: "pitch", yaw_deg: "yaw", roll_deg: "roll" };
      return {
        field, label: t(`settings.vrOverlay.${labelKeys[field]}`),
        value: field.endsWith("_deg") ? `${value.toFixed(0)}°` : `${value.toFixed(2)} m`,
        can_decrease: value > range.min, can_increase: value < range.max,
      };
    });
    const choices = dashboardLanguageChoices(settings, profiles);
    const languageName = (code: string) => code === "auto" ? t("languages.auto") : localizedLanguageName(code, i18n.resolvedLanguage ?? "en-US");
    const targetChoices = (key: "speaker_targets" | "microphone_targets") => settings.translation[key].map((target, index) => ({
      value: languageName(target.target_language),
      options: choices[key][index].map((value) => ({ value, label: languageName(value) })),
    }));
    const strategyKeys = { preferred_only: "preferredOnly", round_robin: "roundRobin", all_languages: "allLanguages" };
    const model: VrDashboardViewModel = {
      labels: {
        ocr_progress: i18n.t("settings.vrOcr.progress", { returnObjects: true }) as Record<string, string>,
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
        position: t("settings.vrOverlay.position"),
        rotation: t("settings.vrOverlay.rotation"),
        reset_position: t("settings.vrDashboard.resetPosition"),
        gesture: t("settings.vrDashboard.gesture"),
        preview: t("settings.vrDashboard.preview"),
        bindings: t("settings.vrDashboard.bindings"),
        saving: t("settings.vrDashboard.saving"),
        saved: t("settings.vrDashboard.saved"),
        display_tab: t("settings.vrDashboard.displayTab"),
        language_tab: t("settings.vrDashboard.languageTab"),
        osc_tab: "OSC",
        recognition_language: t("settings.recognition.language"),
        translation_mode: t("settings.translation.mode"),
        translation_languages: t("settings.translation.targetLanguageSettings"),
        speaker_language: t("settings.translation.targetLanguageForOtherParty"),
        microphone_language: t("settings.translation.targetLanguageForSelf"),
        add_target: t("settings.translation.addRoute"),
        presets: t("settings.translation.presets"),
        save_preset: t("settings.translation.savePreset"),
        apply_preset: t("settings.translation.applyPreset"),
        delete: t("common.delete"),
        osc_original: t("settings.osc.preserveOriginal"),
        osc_enabled: t("settings.osc.enable"),
        osc_mute_sync: t("settings.osc.muteSync"),
        osc_mute_toast: t("settings.osc.muteToast"),
        osc_strategy: t("settings.osc.translationStrategy"),
        osc_hint: t("settings.vrDashboard.oscHint"),
        close: t("common.close"),
      },
      enabled: overlay.enabled,
      ocr_available: settings.features.ocr,
      osc_available: settings.features.osc_chatbox,
      headset: {
        enabled: overlay.headset.enabled,
        content: t(overlay.translation_display === "all_languages"
          ? "settings.vrOverlay.translationDisplays.allLanguages"
          : "settings.vrOverlay.translationDisplays.preferredOnly"),
        width: `${overlay.headset.width_m.toFixed(2)} m`,
        opacity: `${Math.round(overlay.headset.opacity * 100)}%`,
        position: positionFields("headset"),
      },
      wrist: {
        enabled: overlay.wrist.enabled,
        hand: t(`settings.vrOverlay.hands.${overlay.wrist.hand}`),
        content: t(overlay.translation_display === "all_languages"
          ? "settings.vrOverlay.translationDisplays.allLanguages"
          : "settings.vrOverlay.translationDisplays.preferredOnly"),
        width: `${overlay.wrist.width_m.toFixed(2)} m`,
        opacity: `${Math.round(overlay.wrist.opacity * 100)}%`,
        position: positionFields("wrist"),
      },
      ocr: {
        enabled: settings.ocr.enabled,
        backend: t(`settings.vrOcr.backends.${settings.ocr.backend}`),
        gesture: settings.ocr.hand_gesture_enabled,
      },
      language: {
        recognition: {
          value: languageName(liveTranslationServiceName(settings.asr.backend) ? "auto" : settings.asr.language),
          options: choices.recognition.map((value) => ({ value, label: languageName(value) })),
        },
        mode: {
          value: t(`settings.translation.modes.${settings.translation.mode}`),
          options: choices.mode.map((value) => ({ value, label: t(`settings.translation.modes.${value}`) })),
        },
        speaker_targets: targetChoices("speaker_targets"),
        microphone_targets: targetChoices("microphone_targets"),
        can_add: profiles.some(supportsTranslation),
        presets: { value: t("settings.translation.applyPreset"), options: settings.language_presets.map((preset) => ({ value: preset.id, label: preset.name })) },
        can_save_preset: settings.language_presets.length < 5,
      },
      osc: {
        enabled: settings.osc.enabled,
        original: settings.osc.preserve_original_text,
        mute_sync: settings.osc.mute_sync_enabled,
        mute_toast: settings.osc.mute_status_toast_enabled,
        strategy: t(`settings.osc.translationStrategies.${strategyKeys[settings.osc.translation_strategy]}`),
        endpoint: `127.0.0.1:${settings.osc.port}`,
      },
      status: status
        ? t(`settings.vrOverlay.runtimeStates.${status.state}`)
        : t("settings.vrDashboard.connecting"),
      save_state: saveState,
      error: saveError,
    };
    void updateVrDashboardView(model);
  }, [i18n.resolvedLanguage, profiles, saveError, saveState, settings, status, t]);
}
