import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { PlayerAudioStatus } from "../core-client/types";
import { playerAudioNotice } from "../capture/player-audio-notice";

import type { VrOverlayStatus } from "../integrations/types";
import type { Settings } from "../settings/types";
import type { ApiProfileView } from "../providers/types";
import { liveTranslationServiceName } from "../providers/recognition-services";
import { localizedLanguageName } from "../translation/translation-languages";
import { supportsTranslation } from "../providers/api-profile-purpose";
import { applyModelDefaults, resolveModelSelection, translationModelSelection } from "../settings/model-selection";
import { supportsVision } from "../settings/vision-settings";
import {
  VR_OVERLAY_APPEARANCE_RANGES,
  VR_OVERLAY_POSITION_RANGES,
  type VrOverlayAppearanceField,
  type VrOverlayDisplayKind,
  type VrOverlayPositionField,
} from "../settings/vr-overlay-settings";
import {
  getVrOverlayStatus,
  hideVrOverlaySample,
  listenVrDashboardActions,
  listenVrOverlayStatus,
  openVrOcrBindings,
  showVrOverlaySample,
  updateVrDashboardView,
  type VrDashboardViewModel,
} from "../integrations/vr-overlay-native";
import { subscribeDashboardControlResults } from "../vr-controls/useVrControlBridge";
import { applyVrDashboardAction, dashboardLanguageChoices, type VrDashboardAction } from "./settings-actions";

type SaveState = VrDashboardViewModel["save_state"];

export function useVrDashboardBridge(
  settings: Settings | null,
  save: (apply: (settings: Settings) => Settings) => Promise<Settings>,
  profiles: ApiProfileView[],
  capture: {
    running: boolean;
    available: boolean;
    toggle: () => Promise<boolean>;
    playerAudio?: PlayerAudioStatus | null;
    networkVisible?: boolean;
    networkRunning?: boolean;
    networkAvailable?: boolean;
  },
  display: { hiddenTracks: readonly string[]; hiddenSourceLanguages: readonly string[] },
  sceneCapture: boolean,
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
  const lastPublishedRef = useRef("");
  const statusState = status?.state ?? null;
  const headsetSampleVisible = status?.headset.sample_visible ?? false;
  const wristSampleVisible = status?.wrist.sample_visible ?? false;
  const captionNotice = settings?.features.vr_overlay
    ? playerAudioNotice(capture.playerAudio, capture.running || Boolean(capture.networkRunning), t)
    : null;
  const captionNoticeKey = captionNotice ? JSON.stringify(captionNotice) : "";
  const captionNoticeRef = useRef(captionNotice);
  captionNoticeRef.current = captionNotice;

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
      // The shared controls are routed by Rust to the application coordinator.
      if (action === "toggle_capture" || action === "toggle_network_capture" || action === "toggle_wrist" || action === "toggle_headset" || action === "toggle_osc") return;
      if (action === "open_ocr_bindings" && !current.features.ocr && !current.features.visual_question_answering) return;
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
      const applied = applyVrDashboardAction(current, action, profilesRef.current);
      if (applied === current) return;
      const next = applyModelDefaults(applied);
      settingsRef.current = next;
      const version = ++saveVersionRef.current;
      setSaveState("saving");
      setSaveError(null);
      try {
        const saved = await saveRef.current((latest) => {
          const updated = applyVrDashboardAction(latest, action, profilesRef.current);
          return updated === latest ? latest : applyModelDefaults(updated);
        });
        if (version !== saveVersionRef.current) return;
        settingsRef.current = saved;
        setSaveState("saved");
      } catch (reason) {
        console.warn("Failed to save VR dashboard action", reason);
        if (version !== saveVersionRef.current) return;
        setSaveState("error");
        setSaveError(t("settings.vrDashboard.saveFailed"));
      }
    };
    void listenVrDashboardActions((action) => {
      void handleAction(action).catch((reason) => {
        console.warn("VR dashboard action handler failed", reason);
        setSaveState("error");
        setSaveError(t("settings.vrDashboard.actionFailed"));
      });
    }).then((next) => {
      if (disposed) next();
      else unlisten = next;
    });
    const stopDashboardControls = subscribeDashboardControlResults((error) => {
      if (disposed || !settingsRef.current?.features.vr_overlay) return;
      if (error) {
        saveVersionRef.current += 1;
        setSaveState("error");
        setSaveError(t("settings.vrDashboard.actionFailed"));
        return;
      }
      setSaveError(null);
      setSaveState((prev) => (prev === "error" ? "idle" : prev));
    });
    return () => {
      disposed = true;
      stopDashboardControls();
      unlisten();
    };
  }, [t]);

  useEffect(() => {
    if (saveState !== "saved") return;
    const timer = window.setTimeout(() => setSaveState("idle"), 2500);
    return () => window.clearTimeout(timer);
  }, [saveState]);

  const locale = i18n.resolvedLanguage ?? "en-US";
  const ocrTargetsKey = settings?.ocr.targets.map((target) => target.target_language).join(",") ?? "";
  const playerIdLabel = settings?.vrchat_player_id?.trim() ?? "";
  const staticLabels = useMemo(() => {
    const languageName = (code: string) =>
      code === "auto" ? t("languages.auto") : localizedLanguageName(code, locale);
    const ocrTargets = ocrTargetsKey ? ocrTargetsKey.split(",") : [];
    return {
      captions: { me: playerIdLabel || t("live.microphoneMe"), other: t("live.speakerOther") },
      ocr_progress: i18n.t("settings.vrOcr.progress", { returnObjects: true }) as Record<string, string>,
      wrist_reader: {
        ...i18n.t("settings.vrOcr.states", { returnObjects: true }) as Record<string, string>,
        ...i18n.t("wristReader", { returnObjects: true }) as Record<string, string>,
        ...Object.fromEntries(ocrTargets.map((code) => [code, languageName(code)])),
      },
      title: t("settings.vrDashboard.title"),
      subtitle: t("settings.vrDashboard.subtitle"),
      master: t("settings.vrDashboard.master"),
      capture_start: t("capture.start"),
      capture_stop: t("capture.stop"),
      network_capture_start: t("capture.networkStart"),
      network_capture_stop: t("capture.networkStop"),
      headset: t("settings.vrDashboard.headset"),
      wrist: t("settings.vrDashboard.wrist"),
      ocr: t("settings.vrDashboard.ocr"),
      vision: t("settings.vrDashboard.vision"),
      content: t("settings.vrOverlay.translationDisplay"),
      hand: t("settings.vrDashboard.hand"),
      width: t("settings.vrDashboard.width"),
      opacity: t("settings.vrDashboard.opacity"),
      position: t("settings.vrOverlay.position"),
      rotation: t("settings.vrOverlay.rotation"),
      reset_position: t("settings.vrDashboard.resetPosition"),
      recenter_dialogue: t("settings.vrOverlay.recenterDialogue"),
      attach_dialogue: t("settings.vrOverlay.anchorWrist"),
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
      osc_strategy: t("settings.osc.translationStrategy"),
      osc_hint: t("settings.vrDashboard.oscHint"),
      close: t("common.close"),
      content_hint: t("settings.vrDashboard.contentHint"),
      headset_hint: t("settings.vrDashboard.headsetHint"),
      tools: t("settings.vrDashboard.tools"),
      vision_unconfigured: t("settings.vrDashboard.visionUnconfigured"),
      ocr_unavailable: t("settings.vrDashboard.ocrUnavailable"),
      hide_preview: t("settings.vrDashboard.hidePreview"),
      confirm_delete: t("settings.vrDashboard.confirmDelete"),
      delete_hint: t("settings.vrDashboard.deleteHint"),
      cancel: t("common.cancel"),
      position_hint: t("settings.vrDashboard.positionHint"),
      rotation_hint: t("settings.vrDashboard.rotationHint"),
      back: t("settings.vrDashboard.back"),
      preferred: t("settings.vrDashboard.preferred"),
      master_off_banner: t("settings.vrDashboard.masterOffBanner"),
      recognition_locked: t("settings.vrDashboard.recognitionLocked"),
      translation_off_hint: t("settings.vrDashboard.translationOffHint"),
      delete_preset_title: t("settings.vrDashboard.deletePresetTitle"),
      no_presets: t("settings.vrDashboard.noPresets"),
      anchor_mode: t("settings.vrOverlay.anchorMode"),
      anchor_world: t("settings.vrOverlay.anchorWorld"),
      follow_view: t("settings.vrOverlay.followView"),
    };
  }, [i18n, locale, ocrTargetsKey, playerIdLabel, t]);

  useEffect(() => {
    if (!settings?.features.vr_overlay) {
      lastPublishedRef.current = "";
      return;
    }
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
    const appearanceFields = (kind: VrOverlayDisplayKind) => {
      const labelKeys: Record<VrOverlayAppearanceField, string> = {
        width_m: "settings.vrDashboard.width",
        opacity: "settings.vrDashboard.opacity",
        font_size_px: "settings.vrOverlay.fontSize",
      };
      return Object.entries(VR_OVERLAY_APPEARANCE_RANGES[kind]).map(([name, range]) => {
        const field = name as VrOverlayAppearanceField;
        const value = (overlay[kind] as unknown as Record<VrOverlayAppearanceField, number>)[field];
        return {
          field,
          label: t(labelKeys[field]),
          value: field === "width_m" ? `${value.toFixed(2)} m`
            : field === "font_size_px" ? `${Math.round(value)} px`
            : `${Math.round(value * 100)}%`,
          can_decrease: value > range.min,
          can_increase: value < range.max,
        };
      });
    };
    const choices = dashboardLanguageChoices(settings, profiles);
    const languageName = (code: string) => code === "auto" ? t("languages.auto") : localizedLanguageName(code, locale);
    const targetChoices = (key: "speaker_targets" | "microphone_targets") => settings.translation[key].map((target, index) => ({
      value: languageName(target.target_language),
      options: choices[key][index].map((value) => ({ value, label: languageName(value) })),
    }));
    const strategyKeys = { preferred_only: "preferredOnly", round_robin: "roundRobin", all_languages: "allLanguages" };
    const visionBinding = settings.model_defaults?.vision
      ? resolveModelSelection(settings.vision, settings.model_defaults.vision)
      : settings.vision;
    const canAddRoute = (settings.model_defaults?.translation
      ? translationModelSelection(settings).backend === "plamo_local"
      : settings.translation.model_selection?.backend === "plamo_local")
      || profiles.some(supportsTranslation);
    const model: VrDashboardViewModel = {
      locale,
      caption_visibility: { hidden_tracks: display.hiddenTracks, hidden_source_languages: display.hiddenSourceLanguages },
      max_visible_players: settings.audio.output.mode === "vrchat_players" ? settings.audio.output.player_audio?.max_visible_players ?? 4 : null,
      caption_notice: captionNoticeRef.current,
      labels: staticLabels,
      enabled: overlay.enabled,
      capture: { running: capture.running, available: capture.available },
      network_capture: {
        visible: capture.networkVisible ?? false,
        running: capture.networkRunning ?? false,
        available: capture.networkAvailable ?? false,
      },
      content: {
        value: t(overlay.translation_display === "all_languages" ? "settings.vrOverlay.translationDisplays.allLanguages" : "settings.vrOverlay.translationDisplays.preferredOnly"),
        options: ["preferred_only", "all_languages"].map((value) => ({ value, label: t(`settings.vrOverlay.translationDisplays.${value === "all_languages" ? "allLanguages" : "preferredOnly"}`) })),
      },
      ocr_available: settings.features.ocr && sceneCapture,
      vision_available: settings.features.visual_question_answering && sceneCapture,
      osc_available: true,
      headset: {
        enabled: overlay.headset.enabled,
        preview_visible: headsetSampleVisible,
        width: `${overlay.headset.width_m.toFixed(2)} m`,
        opacity: `${Math.round(overlay.headset.opacity * 100)}%`,
        position: positionFields("headset"),
        appearance: appearanceFields("headset"),
      },
      wrist: {
        enabled: overlay.wrist.enabled,
        anchor_mode: overlay.wrist.anchor_mode,
        follow_view: overlay.wrist.follow_view,
        preview_visible: wristSampleVisible,
        hand: {
          value: t(`settings.vrDashboard.hands.${overlay.wrist.hand}`),
          options: ["left", "right", "dominant"].map((value) => ({ value, label: t(`settings.vrDashboard.hands.${value}`) })),
        },
        width: `${overlay.wrist.width_m.toFixed(2)} m`,
        opacity: `${Math.round(overlay.wrist.opacity * 100)}%`,
        position: positionFields("wrist"),
        appearance: appearanceFields("wrist"),
      },
      ocr: {
        enabled: settings.ocr.master_enabled && settings.ocr.enabled,
        backend: t("settings.vrOcr.localModel"),
        available: settings.ocr.master_enabled,
        gesture: settings.ocr.hand_gesture_enabled,
      },
      vision: {
        enabled: settings.vision.enabled && settings.vision.vr_enabled,
        configured: settings.vision.enabled && Boolean(visionBinding.model.trim() && profiles.some((profile) =>
          profile.id === visionBinding.profile_id && supportsVision(profile))),
        model: visionBinding.model,
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
        can_add: canAddRoute,
        presets: { value: t("settings.translation.applyPreset"), options: settings.language_presets.map((preset) => ({ value: preset.id, label: preset.name })) },
        can_save_preset: settings.language_presets.length < 5,
      },
      osc: {
        enabled: settings.osc.enabled,
        original: settings.osc.preserve_original_text,
        mute_sync: settings.osc.mute_sync_enabled,
        strategy: {
          value: t(`settings.osc.translationStrategies.${strategyKeys[settings.osc.translation_strategy]}`),
          options: (Object.keys(strategyKeys) as Array<keyof typeof strategyKeys>).map((value) => ({ value, label: t(`settings.osc.translationStrategies.${strategyKeys[value]}`) })),
        },
        endpoint: `127.0.0.1:${settings.osc.port}`,
      },
      status: statusState
        ? t(`settings.vrOverlay.runtimeStates.${statusState}`)
        : t("settings.vrDashboard.connecting"),
      save_state: saveState,
      error: saveError,
    };
    const serialized = JSON.stringify(model);
    if (serialized === lastPublishedRef.current) return;
    lastPublishedRef.current = serialized;
    void updateVrDashboardView(model);
  }, [capture.running, capture.available, capture.networkVisible, capture.networkRunning, capture.networkAvailable, captionNoticeKey, display.hiddenTracks, display.hiddenSourceLanguages, headsetSampleVisible, locale, profiles, saveError, saveState, sceneCapture, settings, staticLabels, statusState, t, wristSampleVisible]);
}
