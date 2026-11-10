import { invoke } from "@tauri-apps/api/core";
import { ChevronDown, ChevronUp, Settings2, SlidersHorizontal } from "lucide-react";
import { useEffect, useState, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";

import type { DesktopPreferences } from "../../shell/desktop-preferences";
import { localeCatalog } from "../../i18n/catalog";
import { useAppTheme } from "../../app/useAppTheme";
import {
  currentReducedMotionPreference,
  subscribeReducedMotionPreference,
  updateReducedMotionPreference,
} from "../../app/app-motion";
import {
  INTERFACE_SCALE_MAX,
  INTERFACE_SCALE_MIN,
  INTERFACE_SCALE_STEP,
  normalizeInterfaceScale,
} from "../../app/interface-scale";
import type { ApiProfileView } from "../../providers/types";
import type { Settings } from "../types";
import type { UiLanguagePreference } from "../../app/ui-language";
import type { ApplySettings, DebugRow } from "../settings-types";
import { PreferenceToggle, Select } from "../SettingsControls";
import { StorageSettingsSection } from "./StorageSettingsSection";
import { FeatureSettingsCard } from "../system/FeatureSettingsCard";
import { DownloadSourceSettings } from "../system/DownloadSourceSettings";
import { SoftwareUpdateSettings } from "../../updates/SoftwareUpdateSettings";
import type { AppUpdaterState } from "../../updates/useAppUpdater";
import { ConversationTitleSettingsSection } from "./ConversationTitleSettingsSection";
import { DebugSettingsSection } from "./DebugSettingsSection";
import { CapabilityNotice, usePlatformCapabilities } from "../../core-client/capabilities";

export function SystemSettingsSection({
  desktopPreferences,
  desktopPreferencesReady,
  uiLanguageSaving,
  desktopMessage,
  uiLanguagePreference,
  interfaceScale,
  onUpdateDesktop,
  onInterfaceScaleChange,
  onUpdateUiLanguage,
  onboardingDisabled,
  onStartOnboarding,
  locale,
  draft,
  applySettings,
  updater,
  profiles,
  onManageProfiles,
  debugRows,
}: {
  desktopPreferences: DesktopPreferences;
  desktopPreferencesReady: boolean;
  uiLanguageSaving: boolean;
  desktopMessage: string;
  uiLanguagePreference: UiLanguagePreference;
  interfaceScale: number;
  onUpdateDesktop: (key: keyof DesktopPreferences, enabled: boolean) => Promise<void>;
  onInterfaceScaleChange: (value: number) => void;
  onUpdateUiLanguage: (preference: UiLanguagePreference) => Promise<void>;
  onboardingDisabled: boolean;
  onStartOnboarding: () => void;
  locale: string;
  draft: Settings;
  applySettings: ApplySettings;
  updater: AppUpdaterState;
  profiles: ApiProfileView[];
  onManageProfiles: () => void;
  debugRows: DebugRow[];
}) {
  const { t } = useTranslation();
  const { core, desktop } = usePlatformCapabilities();
  const { themePreference, setThemePreference } = useAppTheme();
  const reducedMotion = useSyncExternalStore(subscribeReducedMotionPreference, currentReducedMotionPreference);
  const [interfaceScaleText, setInterfaceScaleText] = useState(() => String(interfaceScale));
  const [shortcutsBusy, setShortcutsBusy] = useState(false);
  const [shortcutResult, setShortcutResult] = useState<{ key: string; shortcuts?: string; error?: boolean } | null>(null);

  useEffect(() => {
    setInterfaceScaleText(String(interfaceScale));
  }, [interfaceScale]);

  const commitInterfaceScale = () => {
    if (!interfaceScaleText.trim() || !Number.isFinite(Number(interfaceScaleText))) {
      setInterfaceScaleText(String(interfaceScale));
      return;
    }
    const next = normalizeInterfaceScale(interfaceScaleText);
    setInterfaceScaleText(String(next));
    if (next !== interfaceScale) onInterfaceScaleChange(next);
  };

  const stepInterfaceScale = (direction: -1 | 1) => {
    const next = normalizeInterfaceScale(interfaceScale + direction * INTERFACE_SCALE_STEP);
    setInterfaceScaleText(String(next));
    if (next !== interfaceScale) onInterfaceScaleChange(next);
  };

  const authorizeShortcuts = async () => {
    if (shortcutsBusy || desktop?.os !== "linux" || !desktop.global_shortcuts.available) return;
    setShortcutsBusy(true);
    setShortcutResult(null);
    try {
      const shortcuts = await invoke<Array<{ id: string; trigger: string }>>("desktop_capture_register_shortcuts");
      setShortcutResult({ key: "granted", shortcuts: shortcuts.map((shortcut) => shortcut.trigger).join(" · ") });
    } catch (error) {
      const code = String(error);
      setShortcutResult({
        key: code === "capture.shortcuts_unavailable" ? "unavailable"
          : code === "capture.shortcuts_disabled" ? "disabled"
            : code === "desktop_vision.shortcut_conflict" ? "conflict" : "notCompleted",
        error: true,
      });
    } finally {
      setShortcutsBusy(false);
    }
  };

  return (
    <div className="settings-section settings-section-active system-section" id="settings-panel-system" role="tabpanel" aria-labelledby="settings-tab-system">
      <div className="section-heading system-page-heading">
        <div><SlidersHorizontal size={18} /><h2>{t("settings.system.title")}</h2></div>
      </div>
      <div className="system-settings-list">
        <section className="system-settings-group" aria-labelledby="system-general-title">
          <div className="section-heading">
            <div><Settings2 size={18} /><h3 id="system-general-title">{t("settings.system.general")}</h3></div>
          </div>
          {desktopMessage && <p className="settings-validation-error" role="alert">{desktopMessage}</p>}
          <div className="system-select-setting">
            <div>
              <strong>{t("settings.system.language")}</strong>
            </div>
            <Select
              label={t("settings.system.language")}
              value={uiLanguagePreference}
              options={[
                { value: "system", label: t("settings.system.followSystem") },
                ...localeCatalog.map(({ _meta }) => ({
                  value: _meta.locale,
                  label: _meta.name,
                })),
              ]}
              disabled={uiLanguageSaving}
              hideLabel
              onChange={(value) => void onUpdateUiLanguage(value as UiLanguagePreference)}
            />
          </div>
          <DownloadSourceSettings source={draft.storage.model_download_source}
            onChange={(source) => applySettings((current) => ({
              ...current, storage: { ...current.storage, model_download_source: source },
            }))} />
          <div className="system-theme-setting">
            <div>
              <strong>{t("settings.system.theme")}</strong>
            </div>
            <div className="theme-toggle-group" role="group" aria-label={t("settings.system.theme")}>
              {([
                ["system", t("settings.system.followSystem")],
                ["light", t("settings.system.themeLight")],
                ["dark", t("settings.system.themeDark")],
              ] as const).map(([value, label]) => (
                <button
                  key={value}
                  type="button"
                  aria-pressed={themePreference === value}
                  className={themePreference === value ? "active" : ""}
                  onClick={() => setThemePreference(value)}
                >
                  {label}
                </button>
              ))}
            </div>
          </div>
          <div className="system-scale-setting">
            <div>
              <strong>{t("settings.system.interfaceScale")}</strong>
            </div>
            <div className="system-scale-input" role="group" aria-label={t("settings.system.interfaceScale")}>
              <span className="system-scale-value">
                <input
                  type="number"
                  inputMode="numeric"
                  min={INTERFACE_SCALE_MIN}
                  max={INTERFACE_SCALE_MAX}
                  step={INTERFACE_SCALE_STEP}
                  value={interfaceScaleText}
                  aria-label={t("settings.system.interfaceScale")}
                  onChange={(event) => {
                    const text = event.currentTarget.value;
                    const value = event.currentTarget.valueAsNumber;
                    setInterfaceScaleText(text);
                    if (
                      Number.isFinite(value)
                      && value >= INTERFACE_SCALE_MIN
                      && value <= INTERFACE_SCALE_MAX
                      && value % INTERFACE_SCALE_STEP === 0
                    ) {
                      onInterfaceScaleChange(value);
                    }
                  }}
                  onBlur={commitInterfaceScale}
                  onKeyDown={(event) => {
                    if (event.key === "ArrowUp" || event.key === "ArrowDown") {
                      event.preventDefault();
                      stepInterfaceScale(event.key === "ArrowUp" ? 1 : -1);
                    }
                    if (event.key === "Enter") event.currentTarget.blur();
                  }}
                />
                <em aria-hidden="true">%</em>
              </span>
              <span className="system-scale-stepper">
                <button
                  type="button"
                  disabled={interfaceScale >= INTERFACE_SCALE_MAX}
                  aria-label={`${t("settings.system.interfaceScale")} +${INTERFACE_SCALE_STEP}%`}
                  data-tooltip={`+${INTERFACE_SCALE_STEP}%`}
                  onClick={() => stepInterfaceScale(1)}
                >
                  <ChevronUp size={13} strokeWidth={2} />
                </button>
                <button
                  type="button"
                  disabled={interfaceScale <= INTERFACE_SCALE_MIN}
                  aria-label={`${t("settings.system.interfaceScale")} -${INTERFACE_SCALE_STEP}%`}
                  data-tooltip={`-${INTERFACE_SCALE_STEP}%`}
                  onClick={() => stepInterfaceScale(-1)}
                >
                  <ChevronDown size={13} strokeWidth={2} />
                </button>
              </span>
            </div>
          </div>
          <div className="settings-toggle-list">
            <PreferenceToggle
              title={t("settings.system.reduceMotion")}
              checked={reducedMotion}
              disabled={false}
              onChange={updateReducedMotionPreference}
            />
            <PreferenceToggle
              title={t("settings.system.launchAtStartup")}
              checked={desktopPreferences.launchAtStartup}
              disabled={!desktopPreferencesReady}
              onChange={(enabled) => void onUpdateDesktop("launchAtStartup", enabled)}
            />
            <PreferenceToggle
              title={t("settings.system.runAsAdministrator")}
              checked={desktopPreferences.runAsAdministrator}
              disabled={!desktopPreferencesReady || core?.os !== "windows"}
              description={core?.os !== "windows" ? t("platform.network_capture_unsupported") : undefined}
              onChange={(enabled) => void onUpdateDesktop("runAsAdministrator", enabled)}
            />
            <PreferenceToggle
              title={t("settings.system.minimizeToTray")}
              checked={desktopPreferences.minimizeToTray}
              disabled={!desktopPreferencesReady}
              onChange={(enabled) => void onUpdateDesktop("minimizeToTray", enabled)}
            />
          </div>
          {desktop?.os === "linux" && desktop.global_shortcuts.supported && (
            <div className="system-onboarding-setting">
              <div>
                <strong>{t("settings.system.shortcutAuthorization.title")}</strong>
                {!desktop.global_shortcuts.available && <CapabilityNotice capability={desktop.global_shortcuts} />}
                {shortcutResult && <p className={shortcutResult.error ? "settings-validation-error" : "settings-inline-message"} role={shortcutResult.error ? "alert" : "status"}>
                  {t(`settings.system.shortcutAuthorization.${shortcutResult.key}`, { shortcuts: shortcutResult.shortcuts })}
                </p>}
              </div>
              <button className="secondary-button" type="button" disabled={shortcutsBusy || !desktop.global_shortcuts.available} onClick={() => void authorizeShortcuts()}>
                {t(shortcutsBusy ? "settings.system.shortcutAuthorization.busy" : "settings.system.shortcutAuthorization.authorize")}
              </button>
            </div>
          )}
          <div className="system-onboarding-setting">
            <div>
              <strong>{t("settings.system.runOnboarding")}</strong>
            </div>
            <button className="secondary-button" type="button" disabled={onboardingDisabled} onClick={onStartOnboarding}>
              {t(onboardingDisabled ? "settings.system.runOnboardingStopFirst" : "settings.system.runOnboardingAction")}
            </button>
          </div>
        </section>
        <ConversationTitleSettingsSection config={draft.conversation_titles} profiles={profiles}
          onChange={(patch) => applySettings((current) => ({
            ...current, conversation_titles: { ...current.conversation_titles, ...patch },
          }))} />
        <FeatureSettingsCard features={draft.features} disabled={false}
          onChange={(key, enabled) => applySettings((current) => ({
            ...current, features: { ...current.features, [key]: enabled },
          }))} />
        <StorageSettingsSection
          locale={locale}
          draft={draft}
          applySettings={applySettings}
        />
        <SoftwareUpdateSettings updater={updater} />
        <DebugSettingsSection rows={debugRows} />
      </div>
    </div>
  );
}
