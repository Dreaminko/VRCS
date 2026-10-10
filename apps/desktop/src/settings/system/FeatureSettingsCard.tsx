import { Blocks, ChevronDown } from "lucide-react";
import { useTranslation } from "react-i18next";

import { PreferenceToggle } from "../SettingsControls";
import type { FeatureKey, FeatureSettings } from "../types";

const featureKeys: FeatureKey[] = ["glossary", "learning", "anki", "osc_chatbox", "ocr", "vr_overlay", "external_api", "vrcx"];

export function FeatureSettingsCard({ features, disabled, onChange }: {
  features: FeatureSettings;
  disabled: boolean;
  onChange: (key: FeatureKey, enabled: boolean) => void;
}) {
  const { t } = useTranslation();
  return (
    <details className="system-settings-group system-features-group" aria-labelledby="system-features-title">
      <summary className="section-heading">
        <div><Blocks size={18} /><h3 id="system-features-title">{t("settings.features.title")}</h3></div>
        <ChevronDown size={16} aria-hidden="true" />
      </summary>
      <div className="settings-toggle-list">
        {featureKeys.map((key) => (
          <PreferenceToggle key={key} title={t(`settings.features.${key}`)}
            checked={features[key]} disabled={disabled} onChange={(enabled) => onChange(key, enabled)} />
        ))}
      </div>
    </details>
  );
}
