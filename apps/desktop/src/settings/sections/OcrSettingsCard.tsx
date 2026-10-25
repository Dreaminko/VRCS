import { useEffect, useRef, useState } from "react";
import { ScanText, Trash2, Download, RefreshCw, Monitor, Glasses, RectangleGoggles } from "lucide-react";
import { useTranslation } from "react-i18next";
import { request } from "../../core-client/transport";
import type { ApiProfileView } from "../../providers/types";
import type { OcrExecutionStatus, VrOcrModelStatus, VrOcrSettings, VrOcrStatus, VrOcrWristSettings } from "../../integrations/types";
import { PreferenceToggle, RangeField, Select } from "../SettingsControls";
import { formatBytes } from "../settings-derived";
import { DEFAULT_OCR_WRIST_SETTINGS, isOcrReady, VR_OVERLAY_POSITION_RANGES } from "../vr-overlay-settings";
import { TranslationRouteList } from "../translation/TranslationRouteList";
import type { DesktopOcrStatus } from "../../ocr/status";
import { ConfirmDialog } from "../../app/components/ConfirmDialog";
import { ShortcutField } from "../../shared/ui/ShortcutField";
import { SettingsDisclosure } from "../components/SettingsDisclosure";

export function OcrSettingsCard({ vrAvailable, config, profiles, disabled: saving, runtime, desktopStatus, onChange }: {
  vrAvailable: boolean;
  config: VrOcrSettings;
  profiles: ApiProfileView[];
  disabled: boolean;
  runtime?: VrOcrStatus;
  desktopStatus: DesktopOcrStatus | null;
  onChange: (patch: Partial<VrOcrSettings>) => void;
}) {
  const { t, i18n } = useTranslation();
  const disabled = saving || !config.master_enabled;
  const [models, setModels] = useState<VrOcrModelStatus | null>(null);
  const [execution, setExecution] = useState<OcrExecutionStatus | null>(null);
  const [modelBusy, setModelBusy] = useState(false);
  const [modelError, setModelError] = useState("");
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [modelRemoving, setModelRemoving] = useState(false);
  const deleteButtonRef = useRef<HTMLButtonElement>(null);
  const locale = i18n.resolvedLanguage ?? "en-US";
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const controller = new AbortController();
    const load = async () => {
      try {
        const next = await request<VrOcrModelStatus>("/api/ocr/models", { signal: controller.signal });
        if (disposed) return;
        setModels(next);
        setModelError("");
        if (next.state === "downloading") timer = setTimeout(() => void load(), 1000);
      } catch {
        if (!disposed) {
          setModelError(t("settings.vrOcr.modelFailed"));
          timer = setTimeout(() => void load(), 3000);
        }
      }
    };
    void load();
    return () => { disposed = true; clearTimeout(timer); controller.abort(); };
  }, [models?.state, t]);

  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const controller = new AbortController();
    const load = async () => {
      try {
        const next = await request<OcrExecutionStatus>("/api/ocr/runtime", { signal: controller.signal });
        if (!disposed) setExecution(next);
      } catch {
        if (!disposed) setExecution(null);
      } finally {
        if (!disposed && config.master_enabled && ((vrAvailable && config.enabled) || config.desktop_enabled)) timer = setTimeout(() => void load(), 2000);
      }
    };
    void load();
    return () => { disposed = true; clearTimeout(timer); controller.abort(); };
  }, [vrAvailable, config.device, config.master_enabled, config.enabled, config.desktop_enabled]);

  const prepareModels = async () => {
    setModelBusy(true);
    setModelError("");
    try {
      setModels(await request<VrOcrModelStatus>("/api/ocr/models/download", { method: "POST" }));
    } catch {
      setModelError(t("settings.vrOcr.modelFailed"));
    } finally {
      setModelBusy(false);
    }
  };

  const deleteModels = async () => {
    if (modelRemoving) return;
    setModelRemoving(true);
    setModelError("");
    try {
      setModels(await request<VrOcrModelStatus>("/api/ocr/models", { method: "DELETE" }));
    } catch {
      setModelError(t("settings.vrOcr.modelDeleteFailed"));
    } finally {
      setModelRemoving(false);
      setConfirmDelete(false);
    }
  };

  const wrist = config.wrist ?? DEFAULT_OCR_WRIST_SETTINGS;
  const updateWrist = (patch: Partial<VrOcrWristSettings>) => onChange({ wrist: { ...wrist, ...patch } });
  const modelProgress = models?.total_bytes ? Math.min(1, Math.max(0, models.downloaded_bytes / models.total_bytes)) : 0;
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
              { value: "directml", label: "GPU (DirectML)" },
            ]}
            disabled={disabled} onChange={(device) => onChange({ device: device as VrOcrSettings["device"] })} />
          {execution?.active_device && <p role="status">
            {t("settings.vrOcr.activeDevice", { device: execution.active_device === "directml" ? "GPU (DirectML)" : "CPU" })}
          </p>}
          {execution?.fallback_reason && <p className="vr-overlay-native-error" role="status" data-tooltip={execution.fallback_reason}>
            {t("settings.vrOcr.cpuFallback")}
          </p>}
          <div className="model-list">
            <div className="model-row">
              <div className="model-row-body">
                <div className="model-row-title">
                  <strong>PP-OCRv6 small</strong>
                  {models && <span className="model-size">{formatBytes(models.downloaded_bytes, locale)} / {formatBytes(models.total_bytes, locale)}</span>}
                </div>
                <p role="status">
                  {models ? t(`settings.vrOcr.modelStates.${models.state}`) : t("common.loading")}
                </p>
                {models?.state === "downloading" && <div className="model-progress-wrap">
                  <div className="model-progress-track" role="progressbar" aria-label={t("settings.recognition.downloadProgress", { name: "PP-OCRv6" })}
                    aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(modelProgress * 100)}>
                    <span style={{ transform: `scaleX(${modelProgress})` }} />
                  </div>
                  <span>{Math.round(modelProgress * 100)}%</span>
                </div>}
                {(modelError || models?.error) && <p className="vr-overlay-native-error" role="alert">{modelError || models?.error}</p>}
              </div>
              <div className="model-row-action">
                {models?.state === "ready" ? <button className="model-delete-button" type="button" ref={deleteButtonRef}
                  disabled={disabled || modelRemoving} aria-label={t("settings.recognition.deleteModel", { name: "PP-OCRv6 small" })}
                  onClick={() => setConfirmDelete(true)}><Trash2 size={16} />{t("common.delete")}</button>
                  : modelBusy || models?.state === "downloading" ? <span className="model-download-state"><RefreshCw size={15} />{t("common.downloading")}</span>
                    : <button className="model-download-button" type="button" disabled={disabled}
                      onClick={() => void prepareModels()}>
                      <Download size={15} />
                      {t(modelError || models?.state === "error" ? "common.retry" : "common.download")}
                    </button>}
              </div>
            </div>
          </div>
          <TranslationRouteList title={t("settings.vrOcr.translation")} targets={config.targets}
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
              {vrState !== "ready" && <span>{t(`settings.vrOcr.states.${vrState}`)}</span>}
              {gestureUnavailable && <span>{t("settings.vrOcr.gestureUnavailable")}</span>}
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
        {confirmDelete && <ConfirmDialog
          title={t("common.delete")}
          message={t("settings.recognition.confirmDelete", { name: "PP-OCRv6 small" })}
          confirmLabel={t("common.delete")}
          busy={modelRemoving}
          icon={<Trash2 size={16} />}
          returnFocusRef={deleteButtonRef}
          onClose={() => setConfirmDelete(false)}
          onConfirm={deleteModels} />}
      </SettingsDisclosure>
    </section>
  );
}
