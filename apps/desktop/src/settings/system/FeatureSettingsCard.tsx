import { Blocks } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { SettingsDisclosure } from "../components/SettingsDisclosure";
import {
  applyFeaturePreset,
  areFeatureSettingsEqual,
  FEATURE_KEYS,
  FEATURE_PRESET_MODES,
  matchFeaturePreset,
  type FeaturePresetMode,
} from "../feature-presets";
import { PreferenceToggle } from "../SettingsControls";
import type { FeatureKey, FeatureSettings } from "../types";
import { usePlatformCapabilities } from "../../core-client/capabilities";

export function FeatureSettingsCard({
  features,
  disabled,
  onUpdate,
}: {
  features: FeatureSettings;
  disabled: boolean;
  onUpdate: (
    update: (current: FeatureSettings) => FeatureSettings,
    afterError?: () => void,
  ) => void;
}) {
  const { t } = useTranslation();
  const { desktop } = usePlatformCapabilities();
  const [manualCustom, setManualCustom] = useState(false);
  const matchedPreset = matchFeaturePreset(features);
  const activeMode: FeaturePresetMode =
    matchedPreset === null || manualCustom ? "custom" : matchedPreset;

  const selectMode = (mode: FeaturePresetMode) => {
    if (disabled) return;
    if (mode === "custom") {
      setManualCustom(true);
      return;
    }
    const previousManualCustom = manualCustom;
    const nextPreview = applyFeaturePreset(features, mode);
    setManualCustom(false);
    if (areFeatureSettingsEqual(features, nextPreview)) return;
    onUpdate(
      (current) => applyFeaturePreset(current, mode),
      () => setManualCustom(previousManualCustom),
    );
  };

  const toggleFeature = (key: FeatureKey, enabled: boolean) => {
    setManualCustom(true);
    onUpdate((current) => ({ ...current, [key]: enabled }));
  };

  return (
    <section className="system-settings-group system-features-group" aria-labelledby="system-features-title">
      <div className="section-heading">
        <div><Blocks size={18} /><h3 id="system-features-title">{t("settings.features.title")}</h3></div>
      </div>
      <div className="system-features-mode-setting">
        <div>
          <strong>{t("settings.features.mode")}</strong>
        </div>
        <div className="theme-toggle-group system-features-mode-group" role="group" aria-label={t("settings.features.mode")}>
          {FEATURE_PRESET_MODES.map((mode) => (
            <button
              key={mode}
              type="button"
              disabled={disabled}
              aria-pressed={activeMode === mode}
              className={activeMode === mode ? "active" : ""}
              onClick={() => selectMode(mode)}
            >
              {t(`settings.features.modes.${mode}`)}
            </button>
          ))}
        </div>
      </div>
      <SettingsDisclosure open={activeMode === "custom"}>
        <div className="settings-toggle-list">
          {FEATURE_KEYS.map((key) => (
            <PreferenceToggle
              key={key}
              title={t(`settings.features.${key}`)}
              checked={features[key]}
              disabled={disabled || (key === "vr_overlay" && !desktop?.vr.subtitles.supported)}
              description={key === "vr_overlay" && desktop?.vr.subtitles.reason_code
                ? t(`platform.${desktop.vr.subtitles.reason_code.replace(/^platform\./, "")}`)
                : undefined}
              onChange={(enabled) => toggleFeature(key, enabled)}
            />
          ))}
        </div>
      </SettingsDisclosure>
    </section>
  );
}
