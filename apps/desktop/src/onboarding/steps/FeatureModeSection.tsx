import { Blocks, Check, Languages, Layers3, SlidersHorizontal } from "lucide-react";
import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { SettingsDisclosure } from "../../settings/components/SettingsDisclosure";
import {
  applyFeaturePreset,
  areFeatureSettingsEqual,
  FEATURE_KEYS,
  FEATURE_PRESET_MODES,
  matchFeaturePreset,
  type FeaturePresetMode,
} from "../../settings/feature-presets";
import { PreferenceToggle } from "../../settings/SettingsControls";
import type { FeatureKey, FeatureSettings } from "../../settings/types";

const MODE_ICONS: Record<FeaturePresetMode, ReactNode> = {
  translation_only: <Languages size={18} />,
  full: <Layers3 size={18} />,
  custom: <SlidersHorizontal size={18} />,
};

export function FeatureModeSection({
  features,
  disabled,
  onUpdateFeatures,
}: {
  features: FeatureSettings;
  disabled: boolean;
  onUpdateFeatures: (update: (current: FeatureSettings) => FeatureSettings) => Promise<boolean>;
}) {
  const { t } = useTranslation();
  const [manualCustom, setManualCustom] = useState(false);
  const matchedPreset = matchFeaturePreset(features);
  const activeMode: FeaturePresetMode =
    matchedPreset === null || manualCustom ? "custom" : matchedPreset;

  const selectMode = async (mode: FeaturePresetMode) => {
    if (disabled) return;
    if (mode === "custom") {
      setManualCustom(true);
      return;
    }
    const previousManualCustom = manualCustom;
    const nextPreview = applyFeaturePreset(features, mode);
    setManualCustom(false);
    if (areFeatureSettingsEqual(features, nextPreview)) return;
    const ok = await onUpdateFeatures((current) => applyFeaturePreset(current, mode));
    if (!ok) setManualCustom(previousManualCustom);
  };

  const toggleFeature = async (key: FeatureKey, enabled: boolean) => {
    if (disabled) return;
    setManualCustom(true);
    await onUpdateFeatures((current) => ({ ...current, [key]: enabled }));
  };

  return (
    <section className="onboarding-feature-section" aria-labelledby="onboarding-feature-mode-title">
      <div className="onboarding-panel-heading">
        <Blocks size={18} />
        <div>
          <strong id="onboarding-feature-mode-title">{t("onboarding.welcome.featureModeTitle")}</strong>
        </div>
      </div>

      <div
        className="onboarding-feature-grid"
        role="group"
        aria-label={t("onboarding.welcome.featureModeTitle")}
      >
        {FEATURE_PRESET_MODES.map((mode) => {
          const selected = activeMode === mode;
          return (
            <button
              key={mode}
              type="button"
              disabled={disabled}
              aria-pressed={selected}
              className={`onboarding-choice onboarding-feature-choice${selected ? " selected" : ""}`}
              onClick={() => void selectMode(mode)}
            >
              {MODE_ICONS[mode]}
              <span>
                <strong>{t(`settings.features.modes.${mode}`)}</strong>
                <small>{t(`onboarding.welcome.featureModes.${mode}.summary`)}</small>
              </span>
              <i>{selected && <Check size={13} />}</i>
            </button>
          );
        })}
      </div>

      <SettingsDisclosure open={activeMode === "custom"}>
        <div className="onboarding-feature-custom">
          <p className="onboarding-feature-custom-hint">
            {t("onboarding.welcome.featureCustomHint")}
          </p>
          <div className="settings-toggle-list onboarding-feature-toggles">
            {FEATURE_KEYS.map((key) => (
              <PreferenceToggle
                key={key}
                title={t(`settings.features.${key}`)}
                checked={features[key]}
                disabled={disabled}
                onChange={(enabled) => void toggleFeature(key, enabled)}
              />
            ))}
          </div>
        </div>
      </SettingsDisclosure>

      <p className="onboarding-feature-footnote">{t("onboarding.welcome.featureModeFootnote")}</p>
    </section>
  );
}
