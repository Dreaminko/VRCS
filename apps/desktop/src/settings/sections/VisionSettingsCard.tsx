import { useEffect, useRef, useState } from "react";
import { ChevronDown, Eye, FlaskConical, Monitor, RectangleGoggles, RefreshCw } from "lucide-react";
import { request } from "../../core-client/transport";
import { localizedError } from "../../app/app-utils";
import { useTranslation } from "react-i18next";
import type { ApiProfileView } from "../../providers/types";
import type { VrOcrWristSettings } from "../../integrations/types";
import { TRANSLATION_LANGUAGE_CODES } from "../../translation/translation-languages";
import { LanguagePicker } from "../../shared/ui/LanguagePicker";
import { ShortcutField } from "../../shared/ui/ShortcutField";
import type { VisionSettings } from "../types";
import { PreferenceToggle, RangeField } from "../SettingsControls";
import { DEFAULT_OCR_WRIST_SETTINGS } from "../vr-overlay-settings";
import { supportsVision, VISION_LIMITS } from "../vision-settings";
import { formatBytes } from "../settings-derived";
import { openDesktopVision } from "../../vision/api";
import { useDesktopVisionSession } from "../../vision/useDesktopVisionSession";
import { SettingsDisclosure } from "../components/SettingsDisclosure";
import { usePlatformCapabilities } from "../../core-client/capabilities";

export function VisionSettingsCard({ config, profiles, vrAvailable, disabled, hideVrSceneCaptureNotice = false, onChange }: {
  config: VisionSettings;
  profiles: ApiProfileView[];
  vrAvailable: boolean;
  disabled: boolean;
  hideVrSceneCaptureNotice?: boolean;
  onChange: (patch: Partial<VisionSettings>) => void;
}) {
  const { t, i18n } = useTranslation();
  const { desktop } = usePlatformCapabilities();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const { status: desktopStatus, statusError } = useDesktopVisionSession();

  const [openError, setOpenError] = useState(false);

  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState("");
  const [testError, setTestError] = useState("");
  const testController = useRef<AbortController | null>(null);
  useEffect(() => {
    const cancelTest = () => {
      const controller = testController.current;
      testController.current = null;
      controller?.abort();
    };
    cancelTest();
    setTesting(false);
    setTestResult("");
    setTestError("");
    return cancelTest;
  }, [config.profile_id, config.model, config.thinking_enabled, config.reasoning_control?.type, config.reasoning_control?.value]);
  const testImage = async () => {
    if (testController.current) return;
    const controller = new AbortController();
    testController.current = controller;
    setTesting(true);
    setTestResult("");
    setTestError("");
    try {
      const result = await request<{ answer: string }>("/api/vision/test", {
        method: "POST", signal: controller.signal,
        timeoutMs: config.request_timeout_seconds * 1000 + 2000,
      });
      if (testController.current === controller && !controller.signal.aborted) setTestResult(result.answer);
    } catch (error) {
      if (testController.current === controller && !controller.signal.aborted) {
        setTestError(localizedError(error, t, "settings.vision.testFailed"));
      }
    } finally {
      if (testController.current === controller) {
        testController.current = null;
        setTesting(false);
      }
    }
  };
  const compatibleProfiles = profiles.filter(supportsVision);
  const selected = compatibleProfiles.find((profile) => profile.id === config.profile_id);
  const wrist = config.wrist ?? DEFAULT_OCR_WRIST_SETTINGS;
  const updateWrist = (patch: Partial<VrOcrWristSettings>) => onChange({ wrist: { ...wrist, ...patch } });
  const connectionSettings = <>
    <LanguagePicker label={t("settings.vision.answerLanguage")} value={config.answer_language}
      languageCodes={TRANSLATION_LANGUAGE_CODES} disabled={disabled}
      onChange={(answer_language) => onChange({ answer_language })} />
    <div className="field vision-settings-model-field">
      <button className="secondary-button" type="button"
        aria-label={t(testing ? "settings.vision.testing" : "settings.vision.testButton")}
        data-tooltip={`${t("settings.vision.testButton")}\n${t("settings.vision.testDescription")}`}
        disabled={disabled || testing || !selected || !config.model.trim()} onClick={() => void testImage()}>
        {testing ? <RefreshCw size={15} className="spin" /> : <FlaskConical size={15} />}
        {t("settings.vision.testButton")}
      </button>
      {testResult && <p className="vision-settings-note vision-settings-result" role="status">{testResult}</p>}
      {testError && <p className="vr-overlay-native-error" role="alert">{testError}</p>}
    </div>
  </>;
  return <section className="vr-overlay-card vision-settings-card" id="vision-question-answering" aria-labelledby="vision-heading">
    <header className="vr-overlay-card-heading vr-overlay-feature-heading">
      <div><Eye size={18} /><span>
        <strong id="vision-heading">{t("settings.vision.title")}</strong>
        <small>{t("settings.vision.description")}</small>
      </span></div>
      <PreferenceToggle title={t("settings.vision.enable")} hideLabel checked={config.enabled}
        disabled={disabled} onChange={(enabled) => onChange({ enabled })} />
    </header>
    <SettingsDisclosure open={config.enabled}>
      <div className="vr-overlay-field-grid">
        {connectionSettings}
      </div>
      <details className="vision-settings-details">
        <summary><span>{t("settings.vision.systemPrompt")}</span><ChevronDown size={16} /></summary>
        <label className="field translation-prompt-field">
          <textarea key={config.system_prompt} aria-label={t("settings.vision.systemPrompt")}
            defaultValue={config.system_prompt} rows={4} maxLength={20000} disabled={disabled}
            onBlur={(event) => { const system_prompt = event.target.value; if (system_prompt.trim() && system_prompt !== config.system_prompt) onChange({ system_prompt }); else event.target.value = config.system_prompt; }} />
        </label>
      </details>
      <details className="vision-settings-details">
        <summary><span>{t("settings.vision.limits")}</span><ChevronDown size={16} /></summary>
        <div className="vr-overlay-range-grid">
          {Object.entries(VISION_LIMITS).filter(([field]) => field !== "display_seconds").map(([field, range]) => <RangeField key={field}
            label={t(`settings.vision.${field}`)} value={config[field as keyof typeof VISION_LIMITS]} {...range}
            disabled={disabled} formatValue={(value) => field === "image_max_bytes" ? formatBytes(value, locale)
              : field === "asr_finalize_timeout_ms" ? `${value} ms` : field.endsWith("seconds") ? `${value} s` : String(value)}
            onCommit={(value) => onChange({ [field]: value })} />)}
        </div>
      </details>
      <section className="vision-settings-group" aria-labelledby="vision-desktop-heading">
        <header className="vr-overlay-card-heading vr-overlay-feature-heading">
          <div><Monitor size={18} /><span><strong id="vision-desktop-heading">{t("settings.vision.desktopTitle")}</strong></span></div>
          <PreferenceToggle title={t("settings.vision.desktopEnable")} hideLabel checked={config.desktop_enabled}
            disabled={disabled || !config.enabled} onChange={(desktop_enabled) => onChange({ desktop_enabled })} />
        </header>
        <SettingsDisclosure open={config.desktop_enabled}>
          <p className="vision-settings-note">{t("settings.vision.desktopDescription")}</p>
          <ShortcutField label={t("settings.vision.desktopShortcut")} value={config.desktop_shortcut} disabled={disabled}

            onChange={(desktop_shortcut) => onChange({ desktop_shortcut })} />

          {desktopStatus?.shortcut_error && <p className="vr-overlay-native-error" role="alert" data-tooltip={desktopStatus.shortcut_error}>
            {t(`visionWindow.errors.${desktopStatus.shortcut_error.replace("desktop_vision.", "")}`, { defaultValue: t("visionWindow.errors.shortcutFailed") })}
          </p>}
          {config.enabled && config.desktop_enabled && desktopStatus?.available === false && <p className="vr-overlay-native-error" role="alert">{t("visionWindow.errors.unavailable")}</p>}
          {(statusError || openError) && <p className="vr-overlay-native-error" role="alert">{t(statusError ? "visionWindow.errors.statusFailed" : "visionWindow.errors.openFailed")}</p>}
          <div className="vr-overlay-card-actions">
            <button className="secondary-button" type="button" disabled={disabled || !config.enabled || !config.desktop_enabled}
              onClick={() => { setOpenError(false); void openDesktopVision().catch(() => setOpenError(true)); }}>
              <Eye size={15} />{t("settings.vision.openDesktop")}
            </button>
          </div>
        </SettingsDisclosure>
      </section>
      <section className="vision-settings-group" aria-labelledby="vision-wrist-heading">
        <header className="vr-overlay-card-heading vr-overlay-feature-heading">
          <div><RectangleGoggles size={18} /><span>
            <strong id="vision-wrist-heading">{t("settings.vision.wristTitle")}</strong>
            {!vrAvailable && !(hideVrSceneCaptureNotice && desktop && !desktop.vr.scene_capture.supported) && (
              <small>
                {desktop && !desktop.vr.scene_capture.supported
                  ? t("platform.vr.scene_capture_unavailable")
                  : t("settings.vision.vrUnavailable")}
              </small>
            )}
          </span></div>
          <PreferenceToggle title={t("settings.vision.vrEnable")} hideLabel checked={config.vr_enabled}
            disabled={disabled || !config.enabled || !vrAvailable} onChange={(vr_enabled) => onChange({ vr_enabled })} />
        </header>
        <SettingsDisclosure open={config.vr_enabled}>
          <RangeField label={t("settings.vision.display_seconds")} value={config.display_seconds}
            {...VISION_LIMITS.display_seconds} disabled={disabled || !vrAvailable} formatValue={(value) => `${value}s`}
            onCommit={(display_seconds) => onChange({ display_seconds })} />
          <p className="vision-settings-note">{t("settings.vrOcr.wristSharedPositionHint")}</p>
          <div className="vr-overlay-field-grid">
            <RangeField label={t("settings.vrOverlay.fontSize")} value={wrist.font_size_px} min={18} max={72} step={1}
              disabled={disabled} formatValue={(value) => `${value}px`} onCommit={(font_size_px) => updateWrist({ font_size_px })} />
            <RangeField label={t("settings.vrOverlay.opacity")} value={wrist.opacity} min={0.1} max={1} step={0.05}
              disabled={disabled} formatValue={(value) => `${Math.round(value * 100)}%`} onCommit={(opacity) => updateWrist({ opacity })} />
          </div>
        </SettingsDisclosure>
      </section>
    </SettingsDisclosure>
  </section>;
}
