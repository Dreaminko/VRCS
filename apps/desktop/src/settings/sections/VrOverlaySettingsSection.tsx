import { useCallback, useEffect, useState } from "react";
import {
  Glasses,
  Hand,
  Layers3,
  RefreshCw,
  RotateCcw,
  TriangleAlert,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import type {
  VrOverlayHeadsetSettings,
  VrOverlayResourceState,
  VrOverlayRuntimeState,
  VrOverlayStatus,
  VrOverlayTranslationDisplay,
  VrOverlayWristSettings,
} from "../../integrations/types";
import type { Settings } from "../types";
import {
  getVrOverlayStatus,
  hideVrOverlaySample,
  listenVrOverlayStatus,
  retryVrOverlay,
  showVrOverlaySample,
  type VrOverlayKind,
} from "../../vr-overlay-native";
import type { ApplySettings, SaveState } from "../settings-types";
import { PreferenceToggle, RangeField, Select } from "../SettingsControls";
import {
  patchVrOverlay,
  patchVrOverlayHeadset,
  patchVrOverlayWrist,
  resetVrOverlayHeadset,
  resetVrOverlayWrist,
  setVrOverlayHeadsetDisplaySeconds,
} from "../vr-overlay-settings";

const runtimeStates: VrOverlayRuntimeState[] = [
  "unsupported",
  "disabled",
  "waiting_runtime",
  "initializing",
  "ready",
  "reconnecting",
  "error",
  "shutting_down",
];

const resourceStates: VrOverlayResourceState[] = [
  "disabled",
  "creating",
  "ready_hidden",
  "visible",
  "fading",
  "device_unavailable",
  "recreating",
  "error",
];

function statusTone(state: VrOverlayRuntimeState | VrOverlayResourceState): string {
  if (state === "ready" || state === "visible") return "ready";
  if (state === "error" || state === "unsupported" || state === "device_unavailable") return "error";
  if (state === "disabled" || state === "ready_hidden") return "muted";
  return "pending";
}

function MeterRange({
  label,
  value,
  min,
  max,
  step,
  disabled,
  unit,
  digits = 0,
  displayMultiplier = 1,
  onCommit,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  disabled: boolean;
  unit: string;
  digits?: number;
  displayMultiplier?: number;
  onCommit: (value: number) => void;
}) {
  const formatValue = (current: number) => `${(current * displayMultiplier).toFixed(digits)}${unit}`;
  return (
    <RangeField
      label={label}
      value={value}
      min={min}
      max={max}
      step={step}
      disabled={disabled}
      formatValue={formatValue}
      onCommit={onCommit}
    />
  );
}

function OverlayStatusBadge({
  state,
  label,
}: {
  state: VrOverlayRuntimeState | VrOverlayResourceState;
  label: string;
}) {
  return <span className={`vr-overlay-status-badge ${statusTone(state)}`} role="status" aria-live="polite">{label}</span>;
}

export function VrOverlaySettingsSection({
  draft,
  saveState,
  applySettings,
}: {
  draft: Settings;
  saveState: SaveState;
  applySettings: ApplySettings;
}) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<VrOverlayStatus | null>(null);
  const [statusError, setStatusError] = useState("");
  const [nativeBusy, setNativeBusy] = useState<"retry" | VrOverlayKind | null>(null);

  const loadStatus = useCallback(async () => {
    try {
      setStatus(await getVrOverlayStatus());
      setStatusError("");
    } catch (reason) {
      setStatusError(reason instanceof Error ? reason.message : t("settings.vrOverlay.statusLoadFailed"));
    }
  }, [t]);

  useEffect(() => {
    let disposed = false;
    let unlisten: () => void = () => undefined;
    void loadStatus();
    void listenVrOverlayStatus((next) => {
      if (!disposed) {
        setStatus(next);
        setStatusError("");
      }
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    }, () => undefined);
    return () => {
      disposed = true;
      unlisten();
    };
  }, [loadStatus]);

  const runNativeAction = async (
    action: "retry" | VrOverlayKind,
    operation: () => Promise<void>,
  ) => {
    setNativeBusy(action);
    setStatusError("");
    try {
      await operation();
      await loadStatus();
    } catch (reason) {
      setStatusError(reason instanceof Error ? reason.message : t("settings.vrOverlay.nativeActionFailed"));
    } finally {
      setNativeBusy(null);
    }
  };

  const toggleSample = (kind: VrOverlayKind, visible: boolean) => {
    void runNativeAction(kind, () => (
      visible ? hideVrOverlaySample(kind) : showVrOverlaySample(kind)
    ));
  };

  const saving = saveState === "saving";
  const overlayDisabled = saving || !draft.vr_overlay.enabled;
  const headsetDisabled = overlayDisabled || !draft.vr_overlay.headset.enabled;
  const wristDisabled = overlayDisabled || !draft.vr_overlay.wrist.enabled;
  const headsetStatus = status?.headset;
  const wristStatus = status?.wrist;

  const runtimeState = status?.state ?? "initializing";
  const runtimeStateKey = runtimeStates.includes(runtimeState)
    ? runtimeState
    : "error";
  const headsetState = headsetStatus?.state ?? "disabled";
  const headsetStateKey = resourceStates.includes(headsetState) ? headsetState : "error";
  const wristState = wristStatus?.state ?? "disabled";
  const wristStateKey = resourceStates.includes(wristState) ? wristState : "error";

  const updateHeadset = <K extends keyof VrOverlayHeadsetSettings>(
    key: K,
    value: VrOverlayHeadsetSettings[K],
  ) => applySettings((current) => patchVrOverlayHeadset(current, { [key]: value }));

  const updateWrist = <K extends keyof VrOverlayWristSettings>(
    key: K,
    value: VrOverlayWristSettings[K],
  ) => applySettings((current) => patchVrOverlayWrist(current, { [key]: value }));

  return (
    <div
      className="settings-section settings-section-active vr-overlay-section"
      id="settings-panel-vr_overlay"
      role="tabpanel"
      aria-labelledby="settings-tab-vr_overlay"
    >
      <div className="section-heading">
        <div><Layers3 size={18} /><h2>{t("settings.vrOverlay.title")}</h2></div>
      </div>

      <div className="vr-overlay-runtime-card">
        <div className="vr-overlay-runtime-main">
          <div>
            <strong>{t("settings.vrOverlay.runtimeTitle")}</strong>
            <span>{t("settings.vrOverlay.runtimeDescription")}</span>
          </div>
          <OverlayStatusBadge
            state={runtimeState}
            label={t(`settings.vrOverlay.runtimeStates.${runtimeStateKey}`)}
          />
        </div>
        <div className="vr-overlay-runtime-facts">
          <span>{t("settings.vrOverlay.runtimeInstalled")}: <strong>{t(status?.runtime_installed ? "common.available" : "common.unavailable")}</strong></span>
          <span>{t("settings.vrOverlay.hmdPresent")}: <strong>{t(status?.hmd_present ? "common.available" : "common.unavailable")}</strong></span>
          {(status?.reconnect_attempt ?? 0) > 0 && <span>{t("settings.vrOverlay.reconnectAttempt", { count: status?.reconnect_attempt })}</span>}
        </div>
        <div className="vr-overlay-runtime-actions">
          <div className="settings-toggle-list">
            <PreferenceToggle
              title={t("settings.vrOverlay.enable")}
              checked={draft.vr_overlay.enabled}
              disabled={saving}
              onChange={(enabled) => applySettings((current) => patchVrOverlay(current, { enabled }))}
            />
          </div>
          <button
            className="secondary-button"
            type="button"
            disabled={nativeBusy !== null || status?.state === "unsupported"}
            onClick={() => void runNativeAction("retry", retryVrOverlay)}
          >
            <RefreshCw size={15} />{t("common.retry")}
          </button>
        </div>
        <Select
          label={t("settings.vrOverlay.translationDisplay")}
          value={draft.vr_overlay.translation_display}
          options={[
            { value: "preferred_only", label: t("settings.vrOverlay.translationDisplays.preferredOnly") },
            { value: "all_languages", label: t("settings.vrOverlay.translationDisplays.allLanguages") },
          ]}
          disabled={overlayDisabled}
          onChange={(value) => applySettings((current) => patchVrOverlay(current, {
            translation_display: value as VrOverlayTranslationDisplay,
          }))}
        />
        {(statusError || status?.last_error_detail) && (
          <p className="vr-overlay-native-error" role="alert">
            <TriangleAlert size={15} />{statusError || status?.last_error_detail}
          </p>
        )}
      </div>

      <div className="vr-overlay-card-list">
        <section className="vr-overlay-card" aria-labelledby="vr-overlay-headset-title">
          <div className="vr-overlay-card-heading">
            <div>
              <Glasses size={18} />
              <span>
                <strong id="vr-overlay-headset-title">{t("settings.vrOverlay.headset.title")}</strong>
                <small>{t("settings.vrOverlay.headset.description")}</small>
              </span>
            </div>
            <OverlayStatusBadge
              state={headsetState}
              label={t(`settings.vrOverlay.resourceStates.${headsetStateKey}`)}
            />
          </div>

          <div className="settings-toggle-list vr-overlay-card-toggles">
            <PreferenceToggle
              title={t("settings.vrOverlay.enableHeadset")}
              checked={draft.vr_overlay.headset.enabled}
              disabled={overlayDisabled}
              onChange={(enabled) => updateHeadset("enabled", enabled)}
            />
          </div>

          <div className="vr-overlay-field-grid">
            <Select
              label={t("settings.vrOverlay.contentMode")}
              value={draft.vr_overlay.headset.content_mode}
              options={[
                { value: "original", label: t("settings.vrOverlay.contentModes.original") },
                { value: "translation", label: t("settings.vrOverlay.contentModes.translation") },
                { value: "bilingual", label: t("settings.vrOverlay.contentModes.bilingual") },
              ]}
              disabled={headsetDisabled}
              onChange={(value) => updateHeadset("content_mode", value as VrOverlayHeadsetSettings["content_mode"])}
            />
            <MeterRange label={t("settings.vrOverlay.displaySeconds")} value={draft.vr_overlay.headset.display_seconds} min={1} max={30} step={0.5} digits={1} unit=" s" disabled={headsetDisabled} onCommit={(value) => applySettings((current) => setVrOverlayHeadsetDisplaySeconds(current, value))} />
            <MeterRange label={t("settings.vrOverlay.fadeSeconds")} value={draft.vr_overlay.headset.fade_seconds} min={0} max={Math.min(5, draft.vr_overlay.headset.display_seconds)} step={0.1} digits={1} unit=" s" disabled={headsetDisabled} onCommit={(value) => updateHeadset("fade_seconds", value)} />
          </div>

          <fieldset className="vr-overlay-toggle-group" disabled={headsetDisabled}>
            <legend>{t("settings.vrOverlay.sources")}</legend>
            <PreferenceToggle title={t("settings.vrOverlay.sourceSpeaker")} checked={draft.vr_overlay.headset.include_speaker} disabled={headsetDisabled} onChange={(value) => updateHeadset("include_speaker", value)} />
            <PreferenceToggle title={t("settings.vrOverlay.sourceMicrophone")} checked={draft.vr_overlay.headset.include_microphone} disabled={headsetDisabled} onChange={(value) => updateHeadset("include_microphone", value)} />
            <PreferenceToggle title={t("settings.vrOverlay.sourceChatbox")} checked={draft.vr_overlay.headset.include_chatbox} disabled={headsetDisabled} onChange={(value) => updateHeadset("include_chatbox", value)} />
          </fieldset>

          <fieldset className="vr-overlay-toggle-group" disabled={headsetDisabled}>
            <legend>{t("settings.vrOverlay.liveUpdates")}</legend>
            <PreferenceToggle title={t("settings.vrOverlay.showRecognitionPartials")} checked={draft.vr_overlay.headset.show_partials} disabled={headsetDisabled} onChange={(value) => updateHeadset("show_partials", value)} />
            <PreferenceToggle title={t("settings.vrOverlay.showTranslationPartials")} checked={draft.vr_overlay.headset.show_translation_partials} disabled={headsetDisabled} onChange={(value) => updateHeadset("show_translation_partials", value)} />
          </fieldset>

          <div className="vr-overlay-range-group">
            <h3>{t("settings.vrOverlay.position")}</h3>
            <div className="vr-overlay-range-grid">
              <MeterRange label={t("settings.vrOverlay.horizontal")} value={draft.vr_overlay.headset.offset_x_m} min={-2} max={2} step={0.01} digits={2} unit=" m" disabled={headsetDisabled} onCommit={(value) => updateHeadset("offset_x_m", value)} />
              <MeterRange label={t("settings.vrOverlay.vertical")} value={draft.vr_overlay.headset.offset_y_m} min={-2} max={2} step={0.01} digits={2} unit=" m" disabled={headsetDisabled} onCommit={(value) => updateHeadset("offset_y_m", value)} />
              <MeterRange label={t("settings.vrOverlay.distance")} value={draft.vr_overlay.headset.distance_m} min={0.25} max={5} step={0.05} digits={2} unit=" m" disabled={headsetDisabled} onCommit={(value) => updateHeadset("distance_m", value)} />
            </div>
          </div>

          <div className="vr-overlay-range-group">
            <h3>{t("settings.vrOverlay.rotation")}</h3>
            <div className="vr-overlay-range-grid">
              <MeterRange label={t("settings.vrOverlay.pitch")} value={draft.vr_overlay.headset.pitch_deg} min={-90} max={90} step={1} unit="°" disabled={headsetDisabled} onCommit={(value) => updateHeadset("pitch_deg", value)} />
              <MeterRange label={t("settings.vrOverlay.yaw")} value={draft.vr_overlay.headset.yaw_deg} min={-180} max={180} step={1} unit="°" disabled={headsetDisabled} onCommit={(value) => updateHeadset("yaw_deg", value)} />
              <MeterRange label={t("settings.vrOverlay.roll")} value={draft.vr_overlay.headset.roll_deg} min={-180} max={180} step={1} unit="°" disabled={headsetDisabled} onCommit={(value) => updateHeadset("roll_deg", value)} />
            </div>
          </div>

          <div className="vr-overlay-range-group">
            <h3>{t("settings.vrOverlay.appearance")}</h3>
            <div className="vr-overlay-range-grid">
              <MeterRange label={t("settings.vrOverlay.width")} value={draft.vr_overlay.headset.width_m} min={0.25} max={3} step={0.05} digits={2} unit=" m" disabled={headsetDisabled} onCommit={(value) => updateHeadset("width_m", value)} />
              <MeterRange label={t("settings.vrOverlay.opacity")} value={draft.vr_overlay.headset.opacity} min={0.1} max={1} step={0.05} unit="%" disabled={headsetDisabled} displayMultiplier={100} onCommit={(value) => updateHeadset("opacity", value)} />
              <MeterRange label={t("settings.vrOverlay.backgroundOpacity")} value={draft.vr_overlay.headset.background_opacity} min={0} max={1} step={0.05} unit="%" disabled={headsetDisabled} displayMultiplier={100} onCommit={(value) => updateHeadset("background_opacity", value)} />
              <MeterRange label={t("settings.vrOverlay.fontSize")} value={draft.vr_overlay.headset.font_size_px} min={24} max={96} step={1} unit=" px" disabled={headsetDisabled} onCommit={(value) => updateHeadset("font_size_px", value)} />
            </div>
          </div>

          <div className="vr-overlay-card-actions">
            <button className="secondary-button" type="button" disabled={headsetDisabled || nativeBusy !== null} aria-pressed={headsetStatus?.sample_visible ?? false} onClick={() => toggleSample("headset", headsetStatus?.sample_visible ?? false)}>
              {headsetStatus?.sample_visible ? t("settings.vrOverlay.hideSample") : t("settings.vrOverlay.showSample")}
            </button>
            <button className="secondary-button" type="button" disabled={headsetDisabled} onClick={() => applySettings(resetVrOverlayHeadset)}>
              <RotateCcw size={15} />{t("settings.vrOverlay.restoreDefaults")}
            </button>
          </div>
        </section>

        <section className="vr-overlay-card" aria-labelledby="vr-overlay-wrist-title">
          <div className="vr-overlay-card-heading">
            <div>
              <Hand size={18} />
              <span>
                <strong id="vr-overlay-wrist-title">{t("settings.vrOverlay.wrist.title")}</strong>
                <small>{t("settings.vrOverlay.wrist.description")}</small>
              </span>
            </div>
            <OverlayStatusBadge
              state={wristState}
              label={t(`settings.vrOverlay.resourceStates.${wristStateKey}`)}
            />
          </div>

          <div className="settings-toggle-list vr-overlay-card-toggles">
            <PreferenceToggle
              title={t("settings.vrOverlay.enableWrist")}
              checked={draft.vr_overlay.wrist.enabled}
              disabled={overlayDisabled}
              onChange={(enabled) => updateWrist("enabled", enabled)}
            />
          </div>

          <div className="vr-overlay-field-grid">
            <Select
              label={t("settings.vrOverlay.hand")}
              value={draft.vr_overlay.wrist.hand}
              options={[
                { value: "left", label: t("settings.vrOverlay.hands.left") },
                { value: "right", label: t("settings.vrOverlay.hands.right") },
                { value: "dominant", label: t("settings.vrOverlay.hands.dominant") },
              ]}
              disabled={wristDisabled}
              onChange={(value) => updateWrist("hand", value as VrOverlayWristSettings["hand"])}
            />
            <Select
              label={t("settings.vrOverlay.dominantHand")}
              value={draft.vr_overlay.wrist.dominant_hand}
              options={[
                { value: "left", label: t("settings.vrOverlay.hands.left") },
                { value: "right", label: t("settings.vrOverlay.hands.right") },
              ]}
              disabled={wristDisabled || draft.vr_overlay.wrist.hand !== "dominant"}
              onChange={(value) => updateWrist("dominant_hand", value as VrOverlayWristSettings["dominant_hand"])}
            />
            <Select
              label={t("settings.vrOverlay.contentMode")}
              value={draft.vr_overlay.wrist.content_mode}
              options={[
                { value: "original", label: t("settings.vrOverlay.contentModes.original") },
                { value: "translation", label: t("settings.vrOverlay.contentModes.translation") },
                { value: "bilingual", label: t("settings.vrOverlay.contentModes.bilingual") },
              ]}
              disabled={wristDisabled}
              onChange={(value) => updateWrist("content_mode", value as VrOverlayWristSettings["content_mode"])}
            />
            <MeterRange label={t("settings.vrOverlay.maxEntries")} value={draft.vr_overlay.wrist.max_entries} min={3} max={10} step={1} unit="" disabled={wristDisabled} onCommit={(value) => updateWrist("max_entries", value)} />
            <MeterRange label={t("settings.vrOverlay.idleHideSeconds")} value={draft.vr_overlay.wrist.idle_hide_seconds} min={0} max={120} step={5} unit=" s" disabled={wristDisabled} onCommit={(value) => updateWrist("idle_hide_seconds", value)} />
          </div>

          <fieldset className="vr-overlay-toggle-group" disabled={wristDisabled}>
            <legend>{t("settings.vrOverlay.sources")}</legend>
            <PreferenceToggle title={t("settings.vrOverlay.sourceSpeaker")} checked={draft.vr_overlay.wrist.include_speaker} disabled={wristDisabled} onChange={(value) => updateWrist("include_speaker", value)} />
            <PreferenceToggle title={t("settings.vrOverlay.sourceMicrophone")} checked={draft.vr_overlay.wrist.include_microphone} disabled={wristDisabled} onChange={(value) => updateWrist("include_microphone", value)} />
            <PreferenceToggle title={t("settings.vrOverlay.sourceChatbox")} checked={draft.vr_overlay.wrist.include_chatbox} disabled={wristDisabled} onChange={(value) => updateWrist("include_chatbox", value)} />
          </fieldset>

          <fieldset className="vr-overlay-toggle-group" disabled={wristDisabled}>
            <legend>{t("settings.vrOverlay.liveUpdates")}</legend>
            <PreferenceToggle title={t("settings.vrOverlay.showRecognitionPartials")} checked={draft.vr_overlay.wrist.show_partials} disabled={wristDisabled} onChange={(value) => updateWrist("show_partials", value)} />
            <PreferenceToggle title={t("settings.vrOverlay.showTranslationPartials")} checked={draft.vr_overlay.wrist.show_translation_partials} disabled={wristDisabled} onChange={(value) => updateWrist("show_translation_partials", value)} />
          </fieldset>

          <div className="vr-overlay-range-group">
            <h3>{t("settings.vrOverlay.position")}</h3>
            <div className="vr-overlay-range-grid">
              <MeterRange label={t("settings.vrOverlay.horizontal")} value={draft.vr_overlay.wrist.offset_x_m} min={-0.5} max={0.5} step={0.01} digits={2} unit=" m" disabled={wristDisabled} onCommit={(value) => updateWrist("offset_x_m", value)} />
              <MeterRange label={t("settings.vrOverlay.vertical")} value={draft.vr_overlay.wrist.offset_y_m} min={-0.5} max={0.5} step={0.01} digits={2} unit=" m" disabled={wristDisabled} onCommit={(value) => updateWrist("offset_y_m", value)} />
              <MeterRange label={t("settings.vrOverlay.depth")} value={draft.vr_overlay.wrist.offset_z_m} min={-0.5} max={0.5} step={0.01} digits={2} unit=" m" disabled={wristDisabled} onCommit={(value) => updateWrist("offset_z_m", value)} />
            </div>
          </div>

          <div className="vr-overlay-range-group">
            <h3>{t("settings.vrOverlay.rotation")}</h3>
            <div className="vr-overlay-range-grid">
              <MeterRange label={t("settings.vrOverlay.pitch")} value={draft.vr_overlay.wrist.pitch_deg} min={-180} max={180} step={1} unit="°" disabled={wristDisabled} onCommit={(value) => updateWrist("pitch_deg", value)} />
              <MeterRange label={t("settings.vrOverlay.yaw")} value={draft.vr_overlay.wrist.yaw_deg} min={-180} max={180} step={1} unit="°" disabled={wristDisabled} onCommit={(value) => updateWrist("yaw_deg", value)} />
              <MeterRange label={t("settings.vrOverlay.roll")} value={draft.vr_overlay.wrist.roll_deg} min={-180} max={180} step={1} unit="°" disabled={wristDisabled} onCommit={(value) => updateWrist("roll_deg", value)} />
            </div>
          </div>

          <div className="vr-overlay-range-group">
            <h3>{t("settings.vrOverlay.appearance")}</h3>
            <div className="vr-overlay-range-grid">
              <MeterRange label={t("settings.vrOverlay.width")} value={draft.vr_overlay.wrist.width_m} min={0.1} max={1} step={0.01} digits={2} unit=" m" disabled={wristDisabled} onCommit={(value) => updateWrist("width_m", value)} />
              <MeterRange label={t("settings.vrOverlay.opacity")} value={draft.vr_overlay.wrist.opacity} min={0.1} max={1} step={0.05} unit="%" disabled={wristDisabled} displayMultiplier={100} onCommit={(value) => updateWrist("opacity", value)} />
              <MeterRange label={t("settings.vrOverlay.backgroundOpacity")} value={draft.vr_overlay.wrist.background_opacity} min={0} max={1} step={0.05} unit="%" disabled={wristDisabled} displayMultiplier={100} onCommit={(value) => updateWrist("background_opacity", value)} />
              <MeterRange label={t("settings.vrOverlay.fontSize")} value={draft.vr_overlay.wrist.font_size_px} min={18} max={72} step={1} unit=" px" disabled={wristDisabled} onCommit={(value) => updateWrist("font_size_px", value)} />
            </div>
          </div>

          <div className="vr-overlay-device-summary">
            <span>{t("settings.vrOverlay.boundRole")}</span>
            <strong>{wristStatus?.bound_role ? t(`settings.vrOverlay.hands.${wristStatus.bound_role}`) : t("common.unavailable")}</strong>
            <span>{t("settings.vrOverlay.trackedDevice")}</span>
            <strong>{t(wristStatus?.tracked_device_available ? "common.available" : "common.unavailable")}</strong>
          </div>

          <div className="vr-overlay-card-actions">
            <button className="secondary-button" type="button" disabled={wristDisabled || nativeBusy !== null} aria-pressed={wristStatus?.sample_visible ?? false} onClick={() => toggleSample("wrist", wristStatus?.sample_visible ?? false)}>
              {wristStatus?.sample_visible ? t("settings.vrOverlay.hideSample") : t("settings.vrOverlay.showSample")}
            </button>
            <button className="secondary-button" type="button" disabled={wristDisabled} onClick={() => applySettings(resetVrOverlayWrist)}>
              <RotateCcw size={15} />{t("settings.vrOverlay.restoreDefaults")}
            </button>
          </div>
        </section>
      </div>
    </div>
  );
}
