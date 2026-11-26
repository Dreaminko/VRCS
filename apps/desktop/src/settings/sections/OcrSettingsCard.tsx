import { useEffect, useState } from "react";
import { ScanText, Monitor, Glasses, RectangleGoggles, ScanEye } from "lucide-react";
import { useTranslation } from "react-i18next";
import { request } from "../../core-client/transport";
import { scanDesktopOcr } from "../../ocr/api";
import { localizedError } from "../../app/app-utils";
import type { ApiProfileView } from "../../providers/types";
import type { OcrExecutionStatus, VrOcrSettings, VrOcrStatus, VrOcrWristSettings } from "../../integrations/types";
import { PreferenceToggle, RangeField, Select } from "../SettingsControls";
import { DEFAULT_OCR_WRIST_SETTINGS, isOcrReady, VR_OVERLAY_POSITION_RANGES } from "../vr-overlay-settings";
import { TranslationRouteList } from "../translation/TranslationRouteList";
import type { DesktopOcrStatus } from "../../ocr/status";
import { ShortcutField } from "../../shared/ui/ShortcutField";
import { SettingsDisclosure } from "../components/SettingsDisclosure";
import { DEFAULT_MODEL_SELECTION, EMPTY_MODEL_BINDING, resolveModelSelection } from "../model-selection";
import type { ModelBinding } from "../types";
import { useOcrModels } from "../ocr/useOcrModels";
import { usePlatformCapabilities } from "../../core-client/capabilities";

export function OcrSettingsCard({ vrAvailable, config, profiles, defaultModel = EMPTY_MODEL_BINDING, disabled: saving, runtime, desktopStatus, onChange }: {
  vrAvailable: boolean;
  config: VrOcrSettings;
  profiles: ApiProfileView[];
  defaultModel?: ModelBinding;
  disabled: boolean;
  runtime?: VrOcrStatus;
  desktopStatus: DesktopOcrStatus | null;
  onChange: (patch: Partial<VrOcrSettings>) => void;
}) {
  const { t } = useTranslation();
  const { core, desktop } = usePlatformCapabilities();
  const disabled = saving || !config.master_enabled;
  const { models } = useOcrModels();
  const [execution, setExecution] = useState<OcrExecutionStatus | null>(null);
  const [captureError, setCaptureError] = useState("");

  useEffect(() => {
    let disposed = false;
    let loaded = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let controller: AbortController | undefined;
    const active = config.master_enabled && ((vrAvailable && config.enabled) || config.desktop_enabled);
    const schedule = () => {
      if (disposed || !active || document.hidden || timer !== undefined) return;
      timer = setTimeout(() => {
        timer = undefined;
        void load();
      }, 2000);
    };
    const load = async () => {
      if (disposed || document.hidden) return;
      if (timer !== undefined) {
        clearTimeout(timer);
        timer = undefined;
      }
      controller?.abort();
      const nextController = new AbortController();
      controller = nextController;
      try {
        const next = await request<OcrExecutionStatus>("/api/ocr/runtime", { signal: nextController.signal });
        if (!disposed && !nextController.signal.aborted) {
          loaded = true;
          setExecution(next);
        }
      } catch {
        if (!disposed && !nextController.signal.aborted) setExecution(null);
      } finally {
        if (!disposed && !nextController.signal.aborted) schedule();
      }
    };
    const onVisibility = () => {
      if (!document.hidden && (active || !loaded)) void load();
    };
    document.addEventListener("visibilitychange", onVisibility);
    if (!document.hidden) void load();
    return () => {
      disposed = true;
      if (timer !== undefined) clearTimeout(timer);
      controller?.abort();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [vrAvailable, config.device, config.master_enabled, config.enabled, config.desktop_enabled]);

  const wrist = config.wrist ?? DEFAULT_OCR_WRIST_SETTINGS;
  const updateWrist = (patch: Partial<VrOcrWristSettings>) => onChange({ wrist: { ...wrist, ...patch } });
  const vrState = runtime?.state ?? "disabled";
  const gestureUnavailable = config.hand_gesture_enabled && !runtime?.gesture_available;
  return (
    <section className="vr-overlay-card" aria-labelledby="ocr-heading">
      <header className="vr-overlay-card-heading vr-overlay-feature-heading">
        <div><ScanText size={18} /><span><strong id="ocr-heading">{t("settings.vrOcr.title")}</strong>
          <small>{t("settings.vrOcr.description")}</small></span></div>
        <PreferenceToggle title={t("settings.ocr.enable")} hideLabel checked={config.master_enabled}
          disabled={saving} onChange={(master_enabled) => onChange({ master_enabled })} />
      </header>
      <SettingsDisclosure open={config.master_enabled}>
        <section className="ocr-settings-group" aria-labelledby="ocr-common-heading">
          <header className="vr-overlay-card-heading">
            <div><span><strong id="ocr-common-heading">{t("settings.ocr.common")}</strong></span></div>
          </header>
          <Select label={t("settings.vrOcr.device")} value={config.device}
            helper={t("settings.vrOcr.deviceDescription")}
            options={[
              { value: "cpu", label: "CPU" },
              { value: "directml", label: "GPU (DirectML)", disabled: !core?.inference.ocr_directml.supported,
                description: !core?.inference.ocr_directml.supported ? t("platform.directml_unsupported") : undefined },
            ]}
            disabled={disabled} onChange={(device) => onChange({ device: device as VrOcrSettings["device"] })} />
          {execution?.active_device && <p role="status">
            {t("settings.vrOcr.activeDevice", { device: execution.active_device === "directml" ? "GPU (DirectML)" : "CPU" })}
          </p>}
          {execution?.fallback_reason && <p className="vr-overlay-native-error" role="status" data-tooltip={execution.fallback_reason}>
            {t("settings.vrOcr.cpuFallback")}
          </p>}
          <TranslationRouteList title={t("settings.vrOcr.translation")} targets={config.targets}
            modelSelection={resolveModelSelection(config.model_selection ?? DEFAULT_MODEL_SELECTION, defaultModel)}
            profiles={profiles} disabled={disabled} onChange={(targets) => onChange({ targets })} />
          <div className="vr-overlay-range-grid">
            <RangeField label={t("settings.vrOcr.timeout")} value={config.timeout_seconds} min={5} max={120} step={5}
              disabled={disabled} formatValue={(value) => `${value}s`} onCommit={(timeout_seconds) => onChange({ timeout_seconds })} />
            <RangeField label={t("settings.vrOcr.confidence")} value={config.minimum_confidence} min={0} max={1} step={0.05}
              disabled={disabled} formatValue={(value) => `${Math.round(value * 100)}%`} onCommit={(minimum_confidence) => onChange({ minimum_confidence })} />
          </div>
        </section>
        <section className="ocr-settings-group" aria-labelledby="ocr-desktop-heading">
          <header className="vr-overlay-card-heading vr-overlay-feature-heading">
            <div><Monitor size={18} /><span><strong id="ocr-desktop-heading">{t("settings.ocr.desktop")}</strong></span></div>
            <PreferenceToggle title={t("settings.ocr.desktopEnabled")} hideLabel checked={config.desktop_enabled}
              disabled={disabled || (!config.desktop_enabled && !isOcrReady(models?.state))}
              onChange={(desktop_enabled) => onChange({ desktop_enabled })} />
          </header>
          <SettingsDisclosure open={config.desktop_enabled}>
            <ShortcutField label={t("settings.ocr.shortcut")} value={config.shortcut} disabled={disabled}
              onChange={(shortcut) => onChange({ shortcut })} />
            {desktopStatus?.shortcut_error && <p className="vr-overlay-native-error" role="alert">
              {t(`ocrWindow.errors.${desktopStatus.shortcut_error.replace("desktop_ocr.", "")}`, { defaultValue: t("ocrWindow.errors.failed") })}
            </p>}
            {captureError && <p className="vr-overlay-native-error" role="alert">{captureError}</p>}
            <div className="vr-overlay-card-actions">
              <button className="secondary-button" type="button"
                disabled={disabled || !config.desktop_enabled || !desktop?.screenshot.available || ["capturing", "selecting", "recognizing", "translating"].includes(desktopStatus?.state ?? "")}
                onClick={() => { setCaptureError(""); void scanDesktopOcr().catch((reason) => setCaptureError(localizedError(reason, t, "ocrWindow.errors.failed"))); }}>
                <ScanEye size={15} />{t("visionWindow.capture")}
              </button>
            </div>
          </SettingsDisclosure>
        </section>
        {vrAvailable && <section className="ocr-settings-group" aria-labelledby="ocr-vr-heading">
          <header className="vr-overlay-card-heading vr-overlay-feature-heading">
            <div><RectangleGoggles size={18} /><span><strong id="ocr-vr-heading">{t("settings.ocr.vr")}</strong></span></div>
            <PreferenceToggle title={t("settings.vrOcr.enable")} hideLabel checked={config.enabled}
              disabled={disabled || (!config.enabled && !isOcrReady(models?.state))}
              onChange={(enabled) => onChange({ enabled })} />
          </header>
          <SettingsDisclosure open={config.enabled}>
            <Select label={t("settings.vrOcr.displayMode")} value={config.display_mode}
              options={[
                { value: "wrist", label: t("settings.vrOcr.displayModes.wrist") },
                { value: "stereo", label: t("settings.vrOcr.displayModes.stereo") },
              ]}
              disabled={disabled} onChange={(display_mode) => onChange({ display_mode: display_mode as VrOcrSettings["display_mode"] })} />
            {(vrState !== "ready" || gestureUnavailable) && <div className="vr-overlay-runtime-facts" role="status" aria-live="polite">
              {vrState !== "ready" && (
                <span className={`vr-overlay-fact ${vrState === "error" ? "is-error" : "is-pending"}`} role={vrState === "error" ? "alert" : undefined}>
                  <i className="vr-overlay-fact-dot" aria-hidden="true" />
                  {t(`settings.vrOcr.states.${vrState}`)}
                </span>
              )}
              {gestureUnavailable && (
                <span className="vr-overlay-fact is-error" role="alert">
                  <i className="vr-overlay-fact-dot" aria-hidden="true" />
                  {t("settings.vrOcr.gestureUnavailable")}
                </span>
              )}
            </div>}
            {(runtime?.last_error_code || runtime?.last_error) && <p className="vr-overlay-native-error" role="alert">
              {t(`settings.vrOcr.errors.${runtime.last_error_code ?? "unavailable"}`, { defaultValue: t("settings.vrOcr.errors.unavailable") })}
            </p>}
            {runtime?.wrist_error && <p className="vr-overlay-native-error" role="alert">{runtime.wrist_error}</p>}
            {runtime && (runtime.completed_translations > 0 || runtime.failed_translations > 0) && <p className="field-description" role="status">
              {t("settings.vrOcr.results", { completed: runtime.completed_translations, failed: runtime.failed_translations })}
            </p>}
            {runtime?.layout_limited && <p className="vr-overlay-native-error" role="status">{t("settings.vrOcr.layoutLimited")}</p>}
            <div className="vr-overlay-range-grid">
              <RangeField label={t("settings.vrOcr.lifetime")} value={config.display_seconds} min={1} max={120} step={1}
                disabled={disabled} formatValue={(value) => `${value}s`} onCommit={(display_seconds) => onChange({ display_seconds })} />
              <RangeField label={t("settings.vrOcr.background")} value={config.background_opacity} min={0} max={1} step={0.05}
                disabled={disabled} formatValue={(value) => `${Math.round(value * 100)}%`} onCommit={(background_opacity) => onChange({ background_opacity })} />
            </div>
            {config.display_mode === "wrist" && <section className="ocr-settings-group" aria-labelledby="ocr-wrist-heading">
              <header className="vr-overlay-card-heading">
                <div><Glasses size={18} /><span><strong id="ocr-wrist-heading">{t("settings.vrOcr.wristTitle")}</strong>
                  <small>{t("settings.vrOcr.wristDescription")}</small></span></div>
              </header>
              <div className="vr-overlay-field-grid">
                <Select label={t("settings.vrOverlay.hand")} value={wrist.hand}
                  options={["left", "right", "dominant"].map((value) => ({ value, label: t(`settings.vrOverlay.hands.${value}`) }))}
                  disabled={disabled} onChange={(hand) => updateWrist({ hand: hand as VrOcrWristSettings["hand"] })} />
                <Select label={t("settings.vrOverlay.dominantHand")} value={wrist.dominant_hand}
                  options={["left", "right"].map((value) => ({ value, label: t(`settings.vrOverlay.hands.${value}`) }))}
                  disabled={disabled || wrist.hand !== "dominant"}
                  onChange={(dominant_hand) => updateWrist({ dominant_hand: dominant_hand as VrOcrWristSettings["dominant_hand"] })} />
                <RangeField label={t("settings.vrOverlay.fontSize")} value={wrist.font_size_px} min={18} max={72} step={1}
                  disabled={disabled} formatValue={(value) => `${value}px`} onCommit={(font_size_px) => updateWrist({ font_size_px })} />
                <RangeField label={t("settings.vrOverlay.width")} value={wrist.width_m} min={0.1} max={1} step={0.01}
                  disabled={disabled} formatValue={(value) => `${value.toFixed(2)}m`} onCommit={(width_m) => updateWrist({ width_m })} />
                <RangeField label={t("settings.vrOverlay.opacity")} value={wrist.opacity} min={0.1} max={1} step={0.05}
                  disabled={disabled} formatValue={(value) => `${Math.round(value * 100)}%`} onCommit={(opacity) => updateWrist({ opacity })} />
              </div>
              <details className="vr-overlay-range-group">
                <summary>{t("settings.vrOcr.wristPosition")}</summary>
                <div className="vr-overlay-range-grid">
                  {([
                    ["offset_x_m", "horizontal"], ["offset_y_m", "vertical"], ["offset_z_m", "depth"],
                    ["pitch_deg", "pitch"], ["yaw_deg", "yaw"], ["roll_deg", "roll"],
                  ] as const).map(([field, label]) => <RangeField key={field} label={t(`settings.vrOverlay.${label}`)}
                    value={wrist[field]} {...VR_OVERLAY_POSITION_RANGES.wrist[field]} disabled={disabled}
                    formatValue={(value) => field.startsWith("offset_") ? `${value.toFixed(2)}m` : `${value}°`}
                    onCommit={(value) => updateWrist({ [field]: value })} />)}
                </div>
              </details>
            </section>}
          </SettingsDisclosure>
        </section>}
      </SettingsDisclosure>
    </section>
  );
}
