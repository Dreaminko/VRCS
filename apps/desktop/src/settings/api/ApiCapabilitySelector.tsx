import { AudioLines, Languages, Sparkles, Image, type LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ReactNode } from "react";

import type { ApiCapability } from "../../providers/types";

const CAPABILITY_OPTIONS: ReadonlyArray<{
  id: ApiCapability;
  icon: LucideIcon;
}> = [
  { id: "speech_to_text", icon: AudioLines },
  { id: "text_generation", icon: Sparkles },
  { id: "text_translation", icon: Languages },
  { id: "image_understanding", icon: Image },
];

export type ApiCapabilityTests = Partial<Record<ApiCapability, ReactNode>>;

export function ApiCapabilitySelector({
  available,
  enabled,
  disabled,
  requiredCapability,
  onToggle,
  tests,
  children,
}: {
  available: ApiCapability[];
  enabled: ApiCapability[];
  disabled: boolean;
  requiredCapability?: ApiCapability;
  onToggle: (capability: ApiCapability) => void;
  tests?: ApiCapabilityTests;
  children?: ReactNode;
}) {
  const { t } = useTranslation();
  const options = CAPABILITY_OPTIONS.filter(({ id }) => available.includes(id));

  return (
    <fieldset className="api-profile-capabilities">
      <legend>{t("settings.apiManagement.capabilities.label")}</legend>
      <p>{t("settings.apiManagement.capabilities.description")}</p>
      <div className="api-profile-capability-list">
        {options.map(({ id, icon: Icon }) => {
          const checked = enabled.includes(id);
          const required = id === requiredCapability;
          const controlDisabled = disabled || required;
          const label = t(`settings.apiManagement.capabilities.${id}`);

          return (
            <div className={`api-profile-capability-row${required ? " required" : ""}`} key={id}>
              <span className="api-profile-capability-icon" aria-hidden="true">
                <Icon size={17} />
              </span>
              <span className="api-profile-capability-copy">
                <span>
                  <strong>{label}</strong>
                  {required && <small className="api-profile-capability-required">{t("settings.apiManagement.capabilities.required")}</small>}
                </span>
                <small>{t(`settings.apiManagement.capabilities.${id}_description`)}</small>
              </span>
              <div className="api-profile-capability-actions">
                {tests?.[id]}
                <button
                  className="settings-switch-button api-profile-capability-switch"
                  type="button"
                  role="switch"
                  aria-checked={checked}
                  aria-label={label}
                  disabled={controlDisabled}
                  onClick={() => onToggle(id)}
                >
                  <span className="switch-track" aria-hidden="true"><span /></span>
                </button>
              </div>
            </div>
          );
        })}
      </div>
      {children}
    </fieldset>
  );
}
