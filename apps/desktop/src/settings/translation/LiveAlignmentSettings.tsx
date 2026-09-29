import { AlignJustify, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { providersApi } from "../../providers/api";
import type { ApiProfileView } from "../../providers/types";
import { useTranslationProfileModels } from "../hooks/useTranslationProfileModels";
import { PreferenceToggle, Select } from "../SettingsControls";
import type { LiveAlignmentSettings as AlignmentSettings } from "../types";

export const DEFAULT_LIVE_ALIGNMENT: AlignmentSettings = {
  enabled: true,
  profile_id: null,
  model: "gpt-6-luna",
  thinking_enabled: false,
};

export function LiveAlignmentSettings({ value = DEFAULT_LIVE_ALIGNMENT, profiles, recognitionProfileId, disabled, onChange }: {
  value?: AlignmentSettings;
  profiles: ApiProfileView[];
  recognitionProfileId: string | null;
  disabled: boolean;
  onChange: (value: AlignmentSettings) => void;
}) {
  const { t } = useTranslation();
  const profile = profiles.find((item) => item.id === (value.profile_id ?? recognitionProfileId));
  const { models, loading, error, refresh } = useTranslationProfileModels(
    profile, value.enabled, providersApi.liveAlignmentModels,
  );
  const modelOptions = [...new Set([
    DEFAULT_LIVE_ALIGNMENT.model,
    value.model,
    ...(loading ? [] : models),
  ].filter(Boolean))];
  return (
    <div className="translation-config-row translation-alignment-row">
      <div className="translation-alignment-heading">
        <AlignJustify size={17} aria-hidden="true" />
        <PreferenceToggle title={t("settings.translation.alignment.enabled")}
          description={t("settings.translation.alignment.description")}
          checked={value.enabled} disabled={disabled}
          onChange={(enabled) => onChange({ ...value, enabled })} />
      </div>
      {value.enabled && (
        <div className="translation-config-fields">
          <Select label={t("settings.translation.alignment.profile")} value={value.profile_id ?? ""}
            disabled={disabled} options={[
              { value: "", label: t("settings.translation.alignment.recognitionProfile") },
              ...profiles.map((p) => ({ value: p.id, label: p.name })),
            ]} onChange={(profile_id) => onChange({
              ...value,
              profile_id: profile_id || null,
              model: DEFAULT_LIVE_ALIGNMENT.model,
            })} />
          <div className="translation-alignment-model">
            <Select label={t("settings.translation.alignment.model")}
              value={value.model || DEFAULT_LIVE_ALIGNMENT.model} disabled={disabled}
              options={modelOptions.map((model) => ({ value: model, label: model }))}
              onChange={(model) => onChange({ ...value, model })} />
            <button className="translation-route-icon-button" type="button"
              aria-label={t("common.refresh")} disabled={disabled || loading || !profile}
              onClick={() => void refresh()}>
              <RefreshCw size={14} className={loading ? "spin" : undefined} />
            </button>
          </div>
          {error && <small className="api-model-catalog-error">{error}</small>}
        </div>
      )}
    </div>
  );
}
