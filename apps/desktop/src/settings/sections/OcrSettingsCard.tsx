import { useEffect, useState } from "react";
import { ScanText, Save, Trash2, Gamepad2, Download, RefreshCw, Monitor, Glasses, Check } from "lucide-react";
import { useTranslation } from "react-i18next";
import { request } from "../../core-client/transport";
import type { CredentialStatus } from "../../shared/protocol/credentials";
import type { ApiProfileView } from "../../providers/types";
import type { VrOcrModelStatus, VrOcrSettings, VrOcrStatus, VrOcrWristSettings } from "../../integrations/types";
import { PreferenceToggle, RangeField, Select } from "../SettingsControls";
import { formatBytes } from "../settings-derived";
import { DEFAULT_OCR_WRIST_SETTINGS, isVrOcrBackendReady, VR_OVERLAY_POSITION_RANGES } from "../vr-overlay-settings";
import { TranslationRouteList } from "../translation/TranslationRouteList";
import type { DesktopOcrStatus } from "../../ocr/status";
import { openVrOcrBindings } from "../../vr-overlay-native";

export function OcrSettingsCard({ config, profiles, disabled, runtime, desktopStatus, onChange }: {
  config: VrOcrSettings;
  profiles: ApiProfileView[];
  disabled: boolean;
  runtime?: VrOcrStatus;
  desktopStatus: DesktopOcrStatus | null;
  onChange: (patch: Partial<VrOcrSettings>) => void;
}) {
  const { t, i18n } = useTranslation();
  const [status, setStatus] = useState<CredentialStatus | null>(null);
  const [shortcut, setShortcut] = useState(config.shortcut);
  useEffect(() => setShortcut(config.shortcut), [config.shortcut]);
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [tokenError, setTokenError] = useState("");
  const [models, setModels] = useState<VrOcrModelStatus | null>(null);
  const [modelBusy, setModelBusy] = useState(false);
  const [modelError, setModelError] = useState("");
  const locale = i18n.resolvedLanguage ?? "en-US";
  useEffect(() => {
    if (config.backend !== "cloud") return;
    let disposed = false;
    void request<CredentialStatus>("/api/ocr/token").then(
      (next) => { if (!disposed) { setStatus(next); setTokenError(""); } },
      () => { if (!disposed) setTokenError(t("settings.vrOcr.tokenFailed")); },
    );
    return () => { disposed = true; };
  }, [config.backend, t]);

  useEffect(() => {
    if (config.backend !== "local") return;
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
  }, [config.backend, models?.state, t]);

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

  const editToken = async (remove: boolean) => {
    setBusy(true);
    setTokenError("");
    try {
      setStatus(await request<CredentialStatus>("/api/ocr/token", {
        method: remove ? "DELETE" : "PUT",
        ...(remove ? {} : { body: JSON.stringify({ token: token.trim() }) }),
      }));
      setToken("");
    } catch {
      setTokenError(t("settings.vrOcr.tokenFailed"));
    } finally {
      setBusy(false);
    }
  };
  const credentialDisabled = disabled || busy || status === null || status.environment_override;
  const local = config.backend === "local";
  const wrist = config.wrist ?? DEFAULT_OCR_WRIST_SETTINGS;
  const updateWrist = (patch: Partial<VrOcrWristSettings>) => onChange({ wrist: { ...wrist, ...patch } });
  const modelProgress = models?.total_bytes ? Math.min(1, Math.max(0, models.downloaded_bytes / models.total_bytes)) : 0;
  return (
    <div className="vr-overlay-card-list">
      <section className="vr-overlay-card" aria-labelledby="ocr-common-heading">
        <header className="vr-overlay-card-heading">
          <div><ScanText size={18} /><span><strong id="ocr-common-heading">{t("settings.ocr.common")}</strong>
            <small>{t(local ? "settings.vrOcr.localDescription" : "settings.vrOcr.description")}</small></span></div>
          <span className="vr-overlay-status-badge muted">PP-OCRv6</span>
        </header>
        <Select label={t("settings.vrOcr.backend")} value={config.backend}
          options={[
            { value: "cloud", label: t("settings.vrOcr.backends.cloud") },
            { value: "local", label: t("settings.vrOcr.backends.local") },
          ]}
          disabled={disabled} onChange={(backend) => onChange({ backend: backend as VrOcrSettings["backend"] })} />
        {local ? <div className="model-list">
          <div className="model-row">
            <div className="model-row-body">
              <div className="model-row-title">
                <strong>PP-OCRv6 small (CPU)</strong>
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
              {models?.state === "ready" ? <span className="model-ready-state"><Check size={15} />{t("common.ready")}</span>
                : modelBusy || models?.state === "downloading" ? <span className="model-download-state"><RefreshCw size={15} />{t("common.downloading")}</span>
                  : <button className="model-download-button" type="button" disabled={disabled}
                    onClick={() => void prepareModels()}>
                    <Download size={15} />
                    {t(modelError || models?.state === "error" ? "common.retry" : "common.download")}
                  </button>}
            </div>
          </div>
        </div> : <div className="external-api-token-section">
          {tokenError && <p className="vr-overlay-native-error" role="alert">{tokenError}</p>}
          <div className="external-api-token-row">
            <strong>{t("settings.vrOcr.token")}</strong>
            <span className={`external-api-token-status ${status?.configured ? "configured" : ""}`}>
              {t(status?.environment_override ? "settings.vrOcr.environment" : status?.configured ? "settings.vrOcr.configured" : "settings.vrOcr.unconfigured")}
            </span>
          </div>
          <label className="external-api-token-input">
            <input type="password" autoComplete="new-password" maxLength={4096} value={token}
              aria-label={t("settings.vrOcr.token")} disabled={credentialDisabled}
              onChange={(event) => setToken(event.target.value)} />
          </label>
          <div className="vr-overlay-card-actions">
            <button className="secondary-button" type="button" disabled={credentialDisabled || !token.trim()}
              onClick={() => void editToken(false)}><Save size={15} />{t("common.save")}</button>
            <button className="secondary-button" type="button" disabled={credentialDisabled || !status?.stored_configured}
              onClick={() => void editToken(true)}><Trash2 size={15} />{t("common.delete")}</button>
          </div>
        </div>}
        <TranslationRouteList title={t("settings.vrOcr.translation")} targets={config.targets}
          profiles={profiles} disabled={disabled} onChange={(targets) => onChange({ targets })} />
        <div className="vr-overlay-range-grid">
          <RangeField label={t("settings.vrOcr.timeout")} value={config.timeout_seconds} min={5} max={120} step={5}
            disabled={disabled} formatValue={(value) => `${value}s`} onCommit={(timeout_seconds) => onChange({ timeout_seconds })} />
          <RangeField label={t("settings.vrOcr.confidence")} value={config.minimum_confidence} min={0} max={1} step={0.05}
            disabled={disabled} formatValue={(value) => `${Math.round(value * 100)}%`} onCommit={(minimum_confidence) => onChange({ minimum_confidence })} />
        </div>
      </section>
      <section className="vr-overlay-card" aria-labelledby="ocr-desktop-heading">
        <header className="vr-overlay-card-heading">
          <div><Monitor size={18} /><span><strong id="ocr-desktop-heading">{t("settings.ocr.desktop")}</strong>
            <small>{t("settings.ocr.desktopResults")}</small></span></div>
        </header>
        <PreferenceToggle title={t("settings.ocr.desktopEnabled")} description={t("settings.ocr.desktopDescription")}
          checked={config.desktop_enabled}
          disabled={disabled || (!config.desktop_enabled && !isVrOcrBackendReady(config.backend, Boolean(status?.configured), models?.state))}
          onChange={(desktop_enabled) => onChange({ desktop_enabled })} />
        <label className="field ocr-shortcut-field">
          <span>{t("settings.ocr.shortcut")}</span>
          <span className="external-api-token-input">
            <input value={shortcut} placeholder="Ctrl+Alt+O" maxLength={64} disabled={disabled} spellCheck={false} autoComplete="off"
              onChange={(event) => setShortcut(event.target.value)}
              onBlur={() => { if (shortcut.trim() && shortcut.trim() !== config.shortcut) onChange({ shortcut: shortcut.trim() }); else setShortcut(config.shortcut); }}
              onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); }} />
          </span>
        </label>
        {desktopStatus?.shortcut_error && <p className="vr-overlay-native-error" role="alert">
          {t(`ocrWindow.errors.${desktopStatus.shortcut_error.replace("desktop_ocr.", "")}`, { defaultValue: t("ocrWindow.errors.failed") })}
        </p>}
      </section>
      <section className="vr-overlay-card" aria-labelledby="ocr-vr-heading">
        <header className="vr-overlay-card-heading">
          <div><Glasses size={18} /><span><strong id="ocr-vr-heading">{t("settings.ocr.vr")}</strong></span></div>
        </header>
        <PreferenceToggle title={t("settings.vrOcr.enable")} checked={config.enabled}
          disabled={disabled || (!config.enabled && !isVrOcrBackendReady(config.backend, Boolean(status?.configured), models?.state))}
          onChange={(enabled) => onChange({ enabled })} />
        <Select label={t("settings.vrOcr.displayMode")} value={config.display_mode}
          options={[
            { value: "wrist", label: t("settings.vrOcr.displayModes.wrist") },
            { value: "stereo", label: t("settings.vrOcr.displayModes.stereo") },
          ]}
          disabled={disabled} onChange={(display_mode) => onChange({ display_mode: display_mode as VrOcrSettings["display_mode"] })} />
        <div className="vr-overlay-runtime-facts" role="status" aria-live="polite">
          <span>{t(`settings.vrOcr.states.${runtime?.state ?? "disabled"}`)}</span>
          {runtime?.controller_bound && <span>{t("settings.vrOcr.controls")}</span>}
          {config.hand_gesture_enabled && <span>{t(runtime?.gesture_available
            ? "settings.vrOcr.gestureReady" : "settings.vrOcr.gestureUnavailable")}</span>}
        </div>
        {(runtime?.last_error_code || runtime?.last_error) && <p className="vr-overlay-native-error" role="alert">
          {t(`settings.vrOcr.errors.${runtime.last_error_code ?? "unavailable"}`, { defaultValue: t("settings.vrOcr.errors.unavailable") })}
        </p>}
        {runtime?.wrist_error && <p className="vr-overlay-native-error" role="alert">{runtime.wrist_error}</p>}
        {runtime && (runtime.completed_translations > 0 || runtime.failed_translations > 0) && <p className="field-description" role="status">
          {t("settings.vrOcr.results", { completed: runtime.completed_translations, failed: runtime.failed_translations })}
        </p>}
        {runtime?.layout_limited && <p className="vr-overlay-native-error" role="status">{t("settings.vrOcr.layoutLimited")}</p>}
        <div className="vr-overlay-card-actions">
          <button className="secondary-button" type="button"
            disabled={disabled || !config.enabled || !runtime || runtime.state === "waiting_vr" || runtime.state === "disabled"}
            onClick={() => { void openVrOcrBindings().catch(() => setError(t("settings.vrOcr.bindingFailed"))); }}>
            <Gamepad2 size={15} />{t("settings.vrOcr.bindings")}
          </button>
        </div>
        {error && <p className="vr-overlay-native-error" role="alert">{error}</p>}
        <div className="vr-overlay-range-grid">
          <RangeField label={t("settings.vrOcr.region")} value={config.region_fraction} min={0.1} max={1} step={0.05}
            disabled={disabled} formatValue={(value) => `${Math.round(value * 100)}%`} onCommit={(region_fraction) => onChange({ region_fraction })} />
          <RangeField label={t("settings.vrOcr.lifetime")} value={config.display_seconds} min={1} max={120} step={1}
            disabled={disabled} formatValue={(value) => `${value}s`} onCommit={(display_seconds) => onChange({ display_seconds })} />
          <RangeField label={t("settings.vrOcr.background")} value={config.background_opacity} min={0} max={1} step={0.05}
            disabled={disabled} formatValue={(value) => `${Math.round(value * 100)}%`} onCommit={(background_opacity) => onChange({ background_opacity })} />
        </div>
        <PreferenceToggle title={t("settings.vrOcr.gesture")} checked={config.hand_gesture_enabled}
          disabled={disabled} onChange={(hand_gesture_enabled) => onChange({ hand_gesture_enabled })} />
      </section>
      {config.display_mode === "wrist" && <section className="vr-overlay-card" aria-labelledby="ocr-wrist-heading">
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
    </div>
  );
}
