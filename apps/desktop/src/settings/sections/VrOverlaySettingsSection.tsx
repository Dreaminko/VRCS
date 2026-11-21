import { useCallback, useEffect, useState } from "react";
import {
  Captions,
  ChevronDown,
  Eye,
  EyeOff,
  Hand,
  Layers3,
  Move,
  RefreshCw,
  RotateCcw,
  SlidersHorizontal,
  TriangleAlert,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import type {
  VrOverlayBackend,
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
  placeVrDialogue,
  showVrOverlaySample,
  type VrOverlayKind,
} from "../../integrations/vr-overlay-native";
import type { ApplySettings, SaveState } from "../settings-types";
import { PreferenceToggle, RangeField, Select } from "../SettingsControls";
import { SettingsDisclosure } from "../components/SettingsDisclosure";
import { CapabilitySection, usePlatformCapabilities } from "../../core-client/capabilities";
import {
  patchVrOverlay,
  patchVrOverlayHeadset,
  patchVrOverlayWrist,
  resetVrOverlayHeadset,
  resetVrOverlayPosition,
  resetVrOverlayWrist,
  setVrOverlayHeadsetDisplaySeconds,
  VR_OVERLAY_POSITION_RANGES,
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

function OverlayToggleColumns({
  disabled,
  includeSpeaker,
  includeMicrophone,
  includeChatbox,
  showPartials,
  showTranslationPartials,
  onChange,
}: {
  disabled: boolean;
  includeSpeaker: boolean;
  includeMicrophone: boolean;
  includeChatbox: boolean;
  showPartials: boolean;
  showTranslationPartials: boolean;
  onChange: (
    key: "include_speaker" | "include_microphone" | "include_chatbox" | "show_partials" | "show_translation_partials",
    value: boolean,
  ) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="vr-overlay-toggle-columns">
      <fieldset className="vr-overlay-toggle-group" disabled={disabled}>
        <legend>{t("settings.vrOverlay.sources")}</legend>
        <PreferenceToggle title={t("settings.vrOverlay.sourceSpeaker")} checked={includeSpeaker} disabled={disabled} onChange={(value) => onChange("include_speaker", value)} />
        <PreferenceToggle title={t("settings.vrOverlay.sourceMicrophone")} checked={includeMicrophone} disabled={disabled} onChange={(value) => onChange("include_microphone", value)} />
        <PreferenceToggle title={t("settings.vrOverlay.sourceChatbox")} checked={includeChatbox} disabled={disabled} onChange={(value) => onChange("include_chatbox", value)} />
      </fieldset>
      <fieldset className="vr-overlay-toggle-group" disabled={disabled}>
        <legend>{t("settings.vrOverlay.liveUpdates")}</legend>
        <PreferenceToggle title={t("settings.vrOverlay.showRecognitionPartials")} checked={showPartials} disabled={disabled} onChange={(value) => onChange("show_partials", value)} />
        <PreferenceToggle title={t("settings.vrOverlay.showTranslationPartials")} checked={showTranslationPartials} disabled={disabled} onChange={(value) => onChange("show_translation_partials", value)} />
      </fieldset>
    </div>
  );
}

function OverlayCardActions({
  disabled,
  sampleDisabled,
  sampleVisible,
  onToggleSample,
  onRestoreDefaults,
}: {
  disabled: boolean;
  sampleDisabled: boolean;
  sampleVisible: boolean;
  onToggleSample: () => void;
  onRestoreDefaults: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="vr-overlay-card-actions">
      <button className="secondary-button vr-overlay-preview-button" type="button" disabled={sampleDisabled} aria-pressed={sampleVisible} onClick={onToggleSample}>
        {sampleVisible ? <EyeOff size={15} aria-hidden="true" /> : <Eye size={15} aria-hidden="true" />}
        {sampleVisible ? t("settings.vrOverlay.hideSample") : t("settings.vrOverlay.showSample")}
      </button>
      <button className="secondary-button" type="button" disabled={disabled} onClick={onRestoreDefaults}>
        <RotateCcw size={15} aria-hidden="true" />{t("settings.vrOverlay.restoreDefaults")}
      </button>
    </div>
  );
}

export function VrOverlaySettingsSection({
  draft,
  os,
  saveState,
  applySettings,
}: {
  draft: Settings;
  os?: string;
  saveState: SaveState;
  applySettings: ApplySettings;
}) {
  const { t } = useTranslation();
  const { desktop } = usePlatformCapabilities();
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

  const overlayDisabled = !draft.vr_overlay.enabled;
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

  const headset = draft.vr_overlay.headset;
  const wrist = draft.vr_overlay.wrist;
  const showDominantHand = wrist.hand === "dominant";
  const wristHandSelect = (
    <Select
      label={t("settings.vrOverlay.hand")}
      value={wrist.hand}
      options={[
        { value: "left", label: t("settings.vrOverlay.hands.left") },
        { value: "right", label: t("settings.vrOverlay.hands.right") },
        { value: "dominant", label: t("settings.vrOverlay.hands.dominant") },
      ]}
      disabled={wristDisabled}
      onChange={(value) => updateWrist("hand", value as VrOverlayWristSettings["hand"])}
    />
  );

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

      <CapabilitySection capability={desktop?.vr.subtitles}>
      <section className="vr-overlay-runtime-card" aria-labelledby="vr-overlay-runtime-title">
        <header className="vr-overlay-card-heading vr-overlay-feature-heading">
          <div>
            <Layers3 size={18} aria-hidden="true" />
            <span>
              <strong id="vr-overlay-runtime-title">{t("settings.vrOverlay.runtimeTitle")}</strong>
            </span>
          </div>
          {draft.vr_overlay.enabled && <OverlayStatusBadge
            state={runtimeState}
            label={t(`settings.vrOverlay.runtimeStates.${runtimeStateKey}`)}
          />}
          <PreferenceToggle
            title={t("settings.vrOverlay.enable")}
            hideLabel
            checked={draft.vr_overlay.enabled}
            disabled={false}
            onChange={(enabled) => applySettings((current) => patchVrOverlay(current, { enabled }))}
          />
        </header>
        <SettingsDisclosure open={draft.vr_overlay.enabled}>
          <div className="vr-overlay-runtime-bar">
            <div className="vr-overlay-runtime-facts">
              <span className={`vr-overlay-fact ${status?.runtime_installed ? "is-ok" : "is-muted"}`}>
                <span className="vr-overlay-fact-dot" aria-hidden="true" />
                <span>{t("settings.vrOverlay.runtimeInstalled")}: <strong>{t(status?.runtime_installed ? "common.available" : "common.unavailable")}</strong></span>
              </span>
              <span className={`vr-overlay-fact ${status?.hmd_present ? "is-ok" : "is-muted"}`}>
                <span className="vr-overlay-fact-dot" aria-hidden="true" />
                <span>{t("settings.vrOverlay.hmdPresent")}: <strong>{t(status?.hmd_present ? "common.available" : "common.unavailable")}</strong></span>
              </span>
              {os === "linux" && (
                <span className={`vr-overlay-fact ${status?.backend ? "is-ok" : "is-muted"}`}>
                  <span className="vr-overlay-fact-dot" aria-hidden="true" />
                  <span>{t("settings.vrOverlay.backend")}: <strong>{t(`settings.vrOverlay.backends.${status?.backend ?? status?.requested_backend ?? "auto"}`)}</strong></span>
                </span>
              )}
              {(status?.reconnect_attempt ?? 0) > 0 && (
                <span className="vr-overlay-fact is-pending">
                  <span className="vr-overlay-fact-dot" aria-hidden="true" />
                  <span>{t("settings.vrOverlay.reconnectAttempt", { count: status?.reconnect_attempt })}</span>
                </span>
              )}
            </div>
            <button
              className="secondary-button"
              type="button"
              disabled={nativeBusy !== null || status?.state === "unsupported"}
              onClick={() => void runNativeAction("retry", retryVrOverlay)}
            >
              <RefreshCw className={nativeBusy === "retry" ? "spinning" : undefined} size={15} aria-hidden="true" />
              {t("common.retry")}
            </button>
          </div>
          {os === "linux" && (
            <div className="vr-overlay-inline-select">
              <Select
                label={t("settings.vrOverlay.backend")}
                value={draft.vr_overlay.backend}
                options={[
                  { value: "auto", label: t("settings.vrOverlay.backends.auto") },
                  { value: "openxr", label: t("settings.vrOverlay.backends.openxr") },
                  { value: "openvr", label: t("settings.vrOverlay.backends.openvr") },
                ]}
                disabled={overlayDisabled}
                onChange={(value) => applySettings((current) => patchVrOverlay(current, {
                  backend: value as VrOverlayBackend,
                }))}
              />
            </div>
          )}
          <div className="vr-overlay-inline-select">
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
          </div>
          {(statusError || status?.last_error_detail) && (
            <p className="vr-overlay-native-error" role="alert">
              <TriangleAlert size={15} aria-hidden="true" />{statusError || status?.last_error_detail}
            </p>
          )}
        </SettingsDisclosure>
      </section>

      <section className="vr-overlay-card" aria-labelledby="vr-overlay-wrist-menu-title">
        <header className="vr-overlay-card-heading vr-overlay-feature-heading">
          <div>
            <SlidersHorizontal size={18} aria-hidden="true" />
            <span>
              <strong id="vr-overlay-wrist-menu-title">{t("settings.vrOverlay.wristMenuEnable")}</strong>
            </span>
          </div>
          <PreferenceToggle
            title={t("settings.vrOverlay.wristMenuEnable")}
            hideLabel
            checked={draft.vr_overlay.wrist_menu.enabled}
            disabled={false}
            onChange={(enabled) => applySettings((current) => patchVrOverlay(current, {
              wrist_menu: { ...current.vr_overlay.wrist_menu, enabled },
            }))}
          />
        </header>
        <SettingsDisclosure open={draft.vr_overlay.wrist_menu.enabled}>
          <div className="vr-overlay-inline-select has-divider">
            <Select label={t("settings.vrOverlay.menuHand")}
              value={draft.vr_overlay.wrist_menu.hand}
              options={["left", "right"].map((value) => ({ value, label: t(`settings.vrDashboard.hands.${value}`) }))}
              disabled={false}
              onChange={(hand) => applySettings((current) => patchVrOverlay(current, {
                wrist_menu: { ...current.vr_overlay.wrist_menu, hand: hand as "left" | "right" },
              }))}
            />
          </div>
        </SettingsDisclosure>
      </section>

      <SettingsDisclosure open={draft.vr_overlay.enabled} className="vr-overlay-card-list">
        <section className="vr-overlay-card" aria-labelledby="vr-overlay-headset-title">
          <header className="vr-overlay-card-heading vr-overlay-feature-heading">
            <div>
              <Captions size={18} aria-hidden="true" />
              <span>
                <strong id="vr-overlay-headset-title">{t("settings.vrOverlay.headset.title")}</strong>
              </span>
            </div>
            {headset.enabled && <OverlayStatusBadge
              state={headsetState}
              label={t(`settings.vrOverlay.resourceStates.${headsetStateKey}`)}
            />}
            <PreferenceToggle
              title={t("settings.vrOverlay.enableHeadset")}
              hideLabel
              checked={headset.enabled}
              disabled={overlayDisabled}
              onChange={(enabled) => updateHeadset("enabled", enabled)}
            />
          </header>

          <SettingsDisclosure open={headset.enabled}>
            <div className="vr-overlay-field-grid is-triple">
              <MeterRange label={t("settings.vrOverlay.linesPerLanguage")} value={headset.lines_per_language} min={1} max={4} step={1} unit="" disabled={headsetDisabled} onCommit={(value) => updateHeadset("lines_per_language", value)} />
              <MeterRange label={t("settings.vrOverlay.displaySeconds")} value={headset.display_seconds} min={1} max={30} step={0.5} digits={1} unit=" s" disabled={headsetDisabled} onCommit={(value) => applySettings((current) => setVrOverlayHeadsetDisplaySeconds(current, value))} />
              <MeterRange label={t("settings.vrOverlay.fadeSeconds")} value={headset.fade_seconds} min={0} max={Math.min(5, headset.display_seconds)} step={0.1} digits={1} unit=" s" disabled={headsetDisabled} onCommit={(value) => updateHeadset("fade_seconds", value)} />
            </div>

            <OverlayToggleColumns
              disabled={headsetDisabled}
              includeSpeaker={headset.include_speaker}
              includeMicrophone={headset.include_microphone}
              includeChatbox={headset.include_chatbox}
              showPartials={headset.show_partials}
              showTranslationPartials={headset.show_translation_partials}
              onChange={(key, value) => updateHeadset(key, value)}
            />

            <div className="vr-overlay-range-group">
              <h3>{t("settings.vrOverlay.appearance")}</h3>
              <div className="vr-overlay-range-grid">
                <MeterRange label={t("settings.vrOverlay.width")} value={headset.width_m} min={0.25} max={3} step={0.05} digits={2} unit=" m" disabled={headsetDisabled} onCommit={(value) => updateHeadset("width_m", value)} />
                <MeterRange label={t("settings.vrOverlay.opacity")} value={headset.opacity} min={0.1} max={1} step={0.05} unit="%" disabled={headsetDisabled} displayMultiplier={100} onCommit={(value) => updateHeadset("opacity", value)} />
                <MeterRange label={t("settings.vrOverlay.backgroundOpacity")} value={headset.background_opacity} min={0} max={1} step={0.05} unit="%" disabled={headsetDisabled} displayMultiplier={100} onCommit={(value) => updateHeadset("background_opacity", value)} />
                <MeterRange label={t("settings.vrOverlay.fontSize")} value={headset.font_size_px} min={24} max={96} step={1} unit=" px" disabled={headsetDisabled} onCommit={(value) => updateHeadset("font_size_px", value)} />
              </div>
            </div>

            <details className="vr-overlay-details">
              <summary className="vr-overlay-details-summary">
                <span>{t("settings.vrOcr.wristPosition")}</span>
                <span className="vr-overlay-details-chips">
                  <span className="vr-overlay-details-chip">{headset.offset_x_m.toFixed(2)}, {headset.offset_y_m.toFixed(2)}, {headset.distance_m.toFixed(2)} m</span>
                  <span className="vr-overlay-details-chip">{headset.pitch_deg}° / {headset.yaw_deg}° / {headset.roll_deg}°</span>
                </span>
                <ChevronDown className="vr-overlay-details-chevron" size={16} aria-hidden="true" />
              </summary>
              <div className="vr-overlay-details-body">
                <div className="vr-overlay-range-group">
                  <h3>{t("settings.vrOverlay.position")}</h3>
                  <div className="vr-overlay-range-grid is-triple">
                    <MeterRange label={t("settings.vrOverlay.horizontal")} value={headset.offset_x_m} {...VR_OVERLAY_POSITION_RANGES.headset.offset_x_m} digits={2} unit=" m" disabled={headsetDisabled} onCommit={(value) => updateHeadset("offset_x_m", value)} />
                    <MeterRange label={t("settings.vrOverlay.vertical")} value={headset.offset_y_m} {...VR_OVERLAY_POSITION_RANGES.headset.offset_y_m} digits={2} unit=" m" disabled={headsetDisabled} onCommit={(value) => updateHeadset("offset_y_m", value)} />
                    <MeterRange label={t("settings.vrOverlay.distance")} value={headset.distance_m} {...VR_OVERLAY_POSITION_RANGES.headset.distance_m} digits={2} unit=" m" disabled={headsetDisabled} onCommit={(value) => updateHeadset("distance_m", value)} />
                  </div>
                </div>
                <div className="vr-overlay-range-group">
                  <h3>{t("settings.vrOverlay.rotation")}</h3>
                  <div className="vr-overlay-range-grid is-triple">
                    <MeterRange label={t("settings.vrOverlay.pitch")} value={headset.pitch_deg} {...VR_OVERLAY_POSITION_RANGES.headset.pitch_deg} unit="°" disabled={headsetDisabled} onCommit={(value) => updateHeadset("pitch_deg", value)} />
                    <MeterRange label={t("settings.vrOverlay.yaw")} value={headset.yaw_deg} {...VR_OVERLAY_POSITION_RANGES.headset.yaw_deg} unit="°" disabled={headsetDisabled} onCommit={(value) => updateHeadset("yaw_deg", value)} />
                    <MeterRange label={t("settings.vrOverlay.roll")} value={headset.roll_deg} {...VR_OVERLAY_POSITION_RANGES.headset.roll_deg} unit="°" disabled={headsetDisabled} onCommit={(value) => updateHeadset("roll_deg", value)} />
                  </div>
                </div>
                <div className="vr-overlay-details-actions">
                  <button className="secondary-button" type="button" disabled={headsetDisabled} onClick={() => applySettings((current) => resetVrOverlayPosition(current, "headset"))}>
                    <RotateCcw size={14} aria-hidden="true" />{t("settings.vrDashboard.resetPosition")}
                  </button>
                </div>
              </div>
            </details>

            <OverlayCardActions
              disabled={headsetDisabled}
              sampleDisabled={headsetDisabled || nativeBusy !== null || saveState === "saving"}
              sampleVisible={headsetStatus?.sample_visible ?? false}
              onToggleSample={() => toggleSample("headset", headsetStatus?.sample_visible ?? false)}
              onRestoreDefaults={() => applySettings(resetVrOverlayHeadset)}
            />
          </SettingsDisclosure>
        </section>

        <section className="vr-overlay-card" aria-labelledby="vr-overlay-wrist-title">
          <header className="vr-overlay-card-heading vr-overlay-feature-heading">
            <div>
              <Hand size={18} aria-hidden="true" />
              <span>
                <strong id="vr-overlay-wrist-title">{t("settings.vrOverlay.wrist.title")}</strong>
              </span>
            </div>
            {wrist.enabled && <OverlayStatusBadge
              state={wristState}
              label={t(`settings.vrOverlay.resourceStates.${wristStateKey}`)}
            />}
            <PreferenceToggle
              title={t("settings.vrOverlay.enableWrist")}
              hideLabel
              checked={wrist.enabled}
              disabled={overlayDisabled}
              onChange={(enabled) => updateWrist("enabled", enabled)}
            />
          </header>

          <SettingsDisclosure open={wrist.enabled}>
            <div className="vr-overlay-field-grid">
              <MeterRange label={t("settings.vrOverlay.maxEntries")} value={wrist.max_entries} min={3} max={10} step={1} unit="" disabled={wristDisabled} onCommit={(value) => updateWrist("max_entries", value)} />
              <RangeField label={t("settings.vrOverlay.idleHideSeconds")} value={wrist.idle_hide_seconds} min={0} max={120} step={5} formatValue={(value) => value === 0 ? t("settings.vrOverlay.keepVisible") : `${value} s`} disabled={wristDisabled} onCommit={(value) => updateWrist("idle_hide_seconds", value)} />
            </div>

            <OverlayToggleColumns
              disabled={wristDisabled}
              includeSpeaker={wrist.include_speaker}
              includeMicrophone={wrist.include_microphone}
              includeChatbox={wrist.include_chatbox}
              showPartials={wrist.show_partials}
              showTranslationPartials={wrist.show_translation_partials}
              onChange={(key, value) => updateWrist(key, value)}
            />

            <div className="vr-overlay-anchor-panel">
              {showDominantHand ? (
                <div className="vr-overlay-field-grid">
                  {wristHandSelect}
                  <Select
                    label={t("settings.vrOverlay.dominantHand")}
                    value={wrist.dominant_hand}
                    options={[
                      { value: "left", label: t("settings.vrOverlay.hands.left") },
                      { value: "right", label: t("settings.vrOverlay.hands.right") },
                    ]}
                    disabled={wristDisabled}
                    onChange={(value) => updateWrist("dominant_hand", value as VrOverlayWristSettings["dominant_hand"])}
                  />
                </div>
              ) : (
                <div className="vr-overlay-inline-select">
                  {wristHandSelect}
                </div>
              )}
              <div className="vr-overlay-anchor-row">
                <Select label={t("settings.vrOverlay.anchorMode")} value={wrist.anchor_mode}
                  options={[{ value: "wrist", label: t("settings.vrOverlay.anchorWrist") }, { value: "world", label: t("settings.vrOverlay.anchorWorld") }]}
                  disabled={wristDisabled} onChange={(value) => updateWrist("anchor_mode", value as "wrist" | "world")} />
                <button className="secondary-button" type="button" disabled={wristDisabled || nativeBusy !== null || status?.state !== "ready"}
                  onClick={() => void runNativeAction("wrist", () => placeVrDialogue("world"))}>
                  <Move size={15} aria-hidden="true" />
                  {t("settings.vrOverlay.recenterDialogue")}
                </button>
              </div>
              {wrist.anchor_mode === "world" && (
                <PreferenceToggle title={t("settings.vrOverlay.followView")} description={t("settings.vrOverlay.followViewHint")}
                  checked={wrist.follow_view} disabled={wristDisabled}
                  onChange={(value) => updateWrist("follow_view", value)} />
              )}
            </div>

            <div className="vr-overlay-range-group">
              <h3>{t("settings.vrOverlay.appearance")}</h3>
              <div className="vr-overlay-range-grid">
                <MeterRange label={t("settings.vrOverlay.width")} value={wrist.width_m} min={0.1} max={1} step={0.01} digits={2} unit=" m" disabled={wristDisabled} onCommit={(value) => updateWrist("width_m", value)} />
                <MeterRange label={t("settings.vrOverlay.opacity")} value={wrist.opacity} min={0.1} max={1} step={0.05} unit="%" disabled={wristDisabled} displayMultiplier={100} onCommit={(value) => updateWrist("opacity", value)} />
                <MeterRange label={t("settings.vrOverlay.backgroundOpacity")} value={wrist.background_opacity} min={0} max={1} step={0.05} unit="%" disabled={wristDisabled} displayMultiplier={100} onCommit={(value) => updateWrist("background_opacity", value)} />
                <MeterRange label={t("settings.vrOverlay.fontSize")} value={wrist.font_size_px} min={18} max={72} step={1} unit=" px" disabled={wristDisabled} onCommit={(value) => updateWrist("font_size_px", value)} />
              </div>
            </div>

            <details className="vr-overlay-details">
              <summary className="vr-overlay-details-summary">
                <span>{t("settings.vrOcr.wristPosition")}</span>
                <span className="vr-overlay-details-chips">
                  <span className="vr-overlay-details-chip">{wrist.offset_x_m.toFixed(2)}, {wrist.offset_y_m.toFixed(2)}, {wrist.offset_z_m.toFixed(2)} m</span>
                  <span className="vr-overlay-details-chip">{wrist.pitch_deg}° / {wrist.yaw_deg}° / {wrist.roll_deg}°</span>
                </span>
                <ChevronDown className="vr-overlay-details-chevron" size={16} aria-hidden="true" />
              </summary>
              <div className="vr-overlay-details-body">
                <div className="vr-overlay-range-group">
                  <h3>{t("settings.vrOverlay.position")}</h3>
                  <div className="vr-overlay-range-grid is-triple">
                    <MeterRange label={t("settings.vrOverlay.horizontal")} value={wrist.offset_x_m} {...VR_OVERLAY_POSITION_RANGES.wrist.offset_x_m} digits={2} unit=" m" disabled={wristDisabled} onCommit={(value) => updateWrist("offset_x_m", value)} />
                    <MeterRange label={t("settings.vrOverlay.vertical")} value={wrist.offset_y_m} {...VR_OVERLAY_POSITION_RANGES.wrist.offset_y_m} digits={2} unit=" m" disabled={wristDisabled} onCommit={(value) => updateWrist("offset_y_m", value)} />
                    <MeterRange label={t("settings.vrOverlay.depth")} value={wrist.offset_z_m} {...VR_OVERLAY_POSITION_RANGES.wrist.offset_z_m} digits={2} unit=" m" disabled={wristDisabled} onCommit={(value) => updateWrist("offset_z_m", value)} />
                  </div>
                </div>
                <div className="vr-overlay-range-group">
                  <h3>{t("settings.vrOverlay.rotation")}</h3>
                  <div className="vr-overlay-range-grid is-triple">
                    <MeterRange label={t("settings.vrOverlay.pitch")} value={wrist.pitch_deg} {...VR_OVERLAY_POSITION_RANGES.wrist.pitch_deg} unit="°" disabled={wristDisabled} onCommit={(value) => updateWrist("pitch_deg", value)} />
                    <MeterRange label={t("settings.vrOverlay.yaw")} value={wrist.yaw_deg} {...VR_OVERLAY_POSITION_RANGES.wrist.yaw_deg} unit="°" disabled={wristDisabled} onCommit={(value) => updateWrist("yaw_deg", value)} />
                    <MeterRange label={t("settings.vrOverlay.roll")} value={wrist.roll_deg} {...VR_OVERLAY_POSITION_RANGES.wrist.roll_deg} unit="°" disabled={wristDisabled} onCommit={(value) => updateWrist("roll_deg", value)} />
                  </div>
                </div>
                <div className="vr-overlay-details-actions">
                  <button className="secondary-button" type="button" disabled={wristDisabled} onClick={() => applySettings((current) => resetVrOverlayPosition(current, "wrist"))}>
                    <RotateCcw size={14} aria-hidden="true" />{t("settings.vrDashboard.resetPosition")}
                  </button>
                </div>
              </div>
            </details>

            <div className="vr-overlay-device-summary">
              <span className={`vr-overlay-fact ${wristStatus?.bound_role ? "is-ok" : "is-muted"}`}>
                <span className="vr-overlay-fact-dot" aria-hidden="true" />
                <span>{t("settings.vrOverlay.boundRole")}: <strong>{wristStatus?.bound_role ? t(`settings.vrOverlay.hands.${wristStatus.bound_role}`) : t("common.unavailable")}</strong></span>
              </span>
              <span className={`vr-overlay-fact ${wristStatus?.tracked_device_available ? "is-ok" : "is-muted"}`}>
                <span className="vr-overlay-fact-dot" aria-hidden="true" />
                <span>{t("settings.vrOverlay.trackedDevice")}: <strong>{t(wristStatus?.tracked_device_available ? "common.available" : "common.unavailable")}</strong></span>
              </span>
            </div>

            <OverlayCardActions
              disabled={wristDisabled}
              sampleDisabled={wristDisabled || nativeBusy !== null || saveState === "saving"}
              sampleVisible={wristStatus?.sample_visible ?? false}
              onToggleSample={() => toggleSample("wrist", wristStatus?.sample_visible ?? false)}
              onRestoreDefaults={() => applySettings(resetVrOverlayWrist)}
            />
          </SettingsDisclosure>
        </section>
      </SettingsDisclosure>
      </CapabilitySection>
    </div>
  );
}
