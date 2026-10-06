import { useEffect, useState } from "react";
import { ScanText, Save, Trash2, Gamepad2, Download, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { request } from "../../core-client/transport";
import type { CredentialStatus } from "../../shared/protocol/credentials";
import type { ApiProfileView } from "../../providers/types";
import type { VrOcrModelStatus, VrOcrSettings, VrOcrStatus } from "../../integrations/types";
import { PreferenceToggle, RangeField, Select } from "../SettingsControls";
import { formatBytes } from "../settings-derived";
import { isVrOcrBackendReady } from "../vr-overlay-settings";
import { TranslationRouteList } from "../translation/TranslationRouteList";
import { openVrOcrBindings } from "../../vr-overlay-native";

export function VrOcrSettingsCard({ config, profiles, disabled, runtime, onChange }: {
  config: VrOcrSettings;
  profiles: ApiProfileView[];
  disabled: boolean;
  runtime?: VrOcrStatus;
  onChange: (patch: Partial<VrOcrSettings>) => void;
}) {
  const { t, i18n } = useTranslation();
  const [status, setStatus] = useState<CredentialStatus | null>(null);
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
  const modelProgress = models?.total_bytes ? Math.min(1, Math.max(0, models.downloaded_bytes / models.total_bytes)) : 0;
  return (
    <section className="vr-overlay-card">
      <header className="vr-overlay-card-heading">
        <div><ScanText size={18} /><span><strong>{t("settings.vrOcr.title")}</strong>
          <small>{t(local ? "settings.vrOcr.localDescription" : "settings.vrOcr.description")}</small></span></div>
        <span className="vr-overlay-status-badge muted">PP-OCRv6</span>
      </header>
      <div className="vr-overlay-runtime-facts" role="status" aria-live="polite">
        <span>{t(`settings.vrOcr.states.${runtime?.state ?? "disabled"}`)}</span>
        {runtime?.controller_bound && <span>{t("settings.vrOcr.controls")}</span>}
        {config.hand_gesture_enabled && <span>{t(runtime?.gesture_available
          ? "settings.vrOcr.gestureReady" : "settings.vrOcr.gestureUnavailable")}</span>}
      </div>
      {(runtime?.last_error_code || runtime?.last_error) && <p className="vr-overlay-native-error" role="alert">
        {t(`settings.vrOcr.errors.${runtime.last_error_code ?? "unavailable"}`, { defaultValue: t("settings.vrOcr.errors.unavailable") })}
      </p>}
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
      <Select label={t("settings.vrOcr.backend")} value={config.backend}
        options={[
          { value: "cloud", label: t("settings.vrOcr.backends.cloud") },
          { value: "local", label: t("settings.vrOcr.backends.local") },
        ]}
        disabled={disabled} onChange={(backend) => onChange({ backend: backend as VrOcrSettings["backend"] })} />
      <Select label={t("settings.vrOcr.displayMode")} value={config.display_mode}
        options={[
          { value: "wrist", label: t("settings.vrOcr.displayModes.wrist") },
          { value: "stereo", label: t("settings.vrOcr.displayModes.stereo") },
        ]}
        disabled={disabled} onChange={(display_mode) => onChange({ display_mode: display_mode as VrOcrSettings["display_mode"] })} />
      <PreferenceToggle title={t("settings.vrOcr.enable")} checked={config.enabled}
        disabled={disabled || (!config.enabled && !isVrOcrBackendReady(config.backend, Boolean(status?.configured), models?.state))}
        onChange={(enabled) => onChange({ enabled })} />
      {local ? <div className="external-api-token-section">
        <div className="external-api-token-row">
          <strong>PP-OCRv6 small (CPU)</strong>
          <span className={`external-api-token-status ${models?.state === "ready" ? "configured" : ""}`} role="status">
            {models ? t(`settings.vrOcr.modelStates.${models.state}`) : t("common.loading")}
          </span>
        </div>
        {models && <span className="model-size">{formatBytes(models.downloaded_bytes, locale)} / {formatBytes(models.total_bytes, locale)}</span>}
        {models?.state === "downloading" && <div className="model-progress-wrap">
          <div className="model-progress-track" role="progressbar" aria-label={t("settings.recognition.downloadProgress", { name: "PP-OCRv6" })}
            aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(modelProgress * 100)}>
            <span style={{ transform: `scaleX(${modelProgress})` }} />
          </div>
          <span>{Math.round(modelProgress * 100)}%</span>
        </div>}
        {(modelError || models?.error) && <p className="vr-overlay-native-error" role="alert">{modelError || models?.error}</p>}
        {models?.state !== "ready" && <div className="vr-overlay-card-actions">
          <button className="secondary-button" type="button" disabled={disabled || modelBusy || models?.state === "downloading"}
            onClick={() => void prepareModels()}>
            {modelBusy || models?.state === "downloading" ? <RefreshCw size={15} /> : <Download size={15} />}
            {t(modelBusy || models?.state === "downloading" ? "common.downloading" : modelError || models?.state === "error" ? "common.retry" : "common.download")}
          </button>
        </div>}
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
      <p className="field-description">{t("settings.vrOcr.sourceView")}</p>
      <RangeField label={t("settings.vrOcr.timeout")} value={config.timeout_seconds} min={5} max={120} step={5}
        disabled={disabled} formatValue={(value) => `${value}s`} onCommit={(timeout_seconds) => onChange({ timeout_seconds })} />
      <RangeField label={t("settings.vrOcr.region")} value={config.region_fraction} min={0.1} max={1} step={0.05}
        disabled={disabled} formatValue={(value) => `${Math.round(value * 100)}%`} onCommit={(region_fraction) => onChange({ region_fraction })} />
      <RangeField label={t("settings.vrOcr.confidence")} value={config.minimum_confidence} min={0} max={1} step={0.05}
        disabled={disabled} formatValue={(value) => `${Math.round(value * 100)}%`} onCommit={(minimum_confidence) => onChange({ minimum_confidence })} />
      <RangeField label={t("settings.vrOcr.lifetime")} value={config.display_seconds} min={1} max={120} step={1}
        disabled={disabled} formatValue={(value) => `${value}s`} onCommit={(display_seconds) => onChange({ display_seconds })} />
      <RangeField label={t("settings.vrOcr.background")} value={config.background_opacity} min={0} max={1} step={0.05}
        disabled={disabled} formatValue={(value) => `${Math.round(value * 100)}%`} onCommit={(background_opacity) => onChange({ background_opacity })} />
      <PreferenceToggle title={t("settings.vrOcr.gesture")} checked={config.hand_gesture_enabled}
        disabled={disabled} onChange={(hand_gesture_enabled) => onChange({ hand_gesture_enabled })} />
    </section>
  );
}
