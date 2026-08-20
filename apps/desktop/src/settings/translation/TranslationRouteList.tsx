import { ArrowDown, ArrowUp, Plus, RefreshCw, Trash2 } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { supportsLlmModels, supportsTranslation } from "../../api-profile-purpose";
import { EditableDropdownField } from "../../shared/ui/DropdownField";
import { LanguagePicker } from "../../shared/ui/LanguagePicker";
import { TRANSLATION_LANGUAGE_CODES } from "../../translation-languages";
import { selectTranslationModel } from "../../translation-model-selection";
import { thinkingControlForModel } from "../../translation-thinking";
import type { ApiProfileView, TranslationTargetSettings } from "../../types";
import { useTranslationProfileModels } from "../hooks/useTranslationProfileModels";
import { PreferenceToggle, Select } from "../SettingsControls";

function profileLabel(profile: ApiProfileView): string {
  return profile.name === profile.provider_display_name
    ? profile.name
    : `${profile.name} · ${profile.provider_display_name}`;
}

export function TranslationRouteList({
  title,
  targets,
  profiles,
  disabled,
  onChange,
}: {
  title: string;
  targets: TranslationTargetSettings[];
  profiles: ApiProfileView[];
  disabled: boolean;
  onChange: (targets: TranslationTargetSettings[]) => void;
}) {
  const { t } = useTranslation();
  const translationProfiles = profiles.filter(supportsTranslation);
  const addTarget = () => {
    if (targets.length >= 3) return;
    const used = new Set(targets.map((target) => target.target_language));
    const profile = translationProfiles[0];
    const targetLanguage = (profile?.capabilities.supported_languages.length
      ? profile.capabilities.supported_languages
      : TRANSLATION_LANGUAGE_CODES
    ).find((code) => !used.has(code)) ?? "en";
    onChange([
      ...targets,
      {
        target_language: targetLanguage,
        profile_id: profile?.id ?? null,
        model: "gpt-5-mini",
        thinking_enabled: false,
      },
    ]);
  };
  const move = (index: number, offset: number) => {
    const nextIndex = index + offset;
    if (nextIndex < 0 || nextIndex >= targets.length) return;
    const next = [...targets];
    [next[index], next[nextIndex]] = [next[nextIndex], next[index]];
    onChange(next);
  };

  return (
    <section className="translation-route-group">
      <header className="translation-route-group-header">
        <div>
          <strong>{title}</strong>
          <small>{t("settings.translation.routeHint")}</small>
        </div>
      </header>
      <div className="translation-route-list">
        {targets.map((target, index) => (
          <TranslationRouteRow
            key={`${target.target_language}-${index}`}
            index={index}
            target={target}
            profiles={translationProfiles}
            usedLanguages={targets.map((item) => item.target_language)}
            disabled={disabled}
            canDelete={targets.length > 1}
            canMoveDown={index < targets.length - 1}
            onChange={(nextTarget) => {
              const next = [...targets];
              next[index] = nextTarget;
              onChange(next);
            }}
            onMoveUp={() => move(index, -1)}
            onMoveDown={() => move(index, 1)}
            onDelete={() => onChange(targets.filter((_, itemIndex) => itemIndex !== index))}
          />
        ))}
      </div>
      <button
        className="secondary-button translation-route-add"
        type="button"
        disabled={disabled || targets.length >= 3 || !translationProfiles.length}
        onClick={addTarget}
      >
        <Plus size={14} />
        {t("settings.translation.addRoute")}
      </button>
    </section>
  );
}

function TranslationRouteRow({
  index,
  target,
  profiles,
  usedLanguages,
  disabled,
  canDelete,
  canMoveDown,
  onChange,
  onMoveUp,
  onMoveDown,
  onDelete,
}: {
  index: number;
  target: TranslationTargetSettings;
  profiles: ApiProfileView[];
  usedLanguages: string[];
  disabled: boolean;
  canDelete: boolean;
  canMoveDown: boolean;
  onChange: (target: TranslationTargetSettings) => void;
  onMoveUp: () => void;
  onMoveDown: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation();
  const [switching, setSwitching] = useState(false);
  const profile = profiles.find((item) => item.id === target.profile_id);
  const usesModels = Boolean(profile && supportsLlmModels(profile));
  const { models, loading, error, refresh, load } = useTranslationProfileModels(profile, usesModels);
  const languageCodes = (profile?.capabilities.supported_languages ?? TRANSLATION_LANGUAGE_CODES)
    .filter((language) => language === target.target_language || !usedLanguages.includes(language));
  const thinkingControl = thinkingControlForModel(profile?.provider, target.model);
  const selectProfile = async (profileId: string) => {
    const nextProfile = profiles.find((item) => item.id === profileId);
    if (!nextProfile) return;
    setSwitching(true);
    try {
      let model = target.model;
      if (supportsLlmModels(nextProfile) && nextProfile.provider !== "openai_compatible") {
        const available = await load(nextProfile.id);
        model = selectTranslationModel(nextProfile.provider, available, target.model) ?? "";
      }
      onChange({ ...target, profile_id: profileId, model });
    } finally {
      setSwitching(false);
    }
  };

  return (
    <div className="translation-route-row">
      <div className="translation-route-rank">
        {index === 0 ? t("settings.translation.preferred") : index + 1}
      </div>
      <div className="translation-route-fields">
        <LanguagePicker
          compact
          label={t("settings.translation.targetLanguageSettings")}
          value={target.target_language}
          languageCodes={languageCodes}
          allowCustom={profile?.capabilities.supports_custom_translation_language ?? false}
          disabled={disabled}
          onChange={(target_language) => onChange({ ...target, target_language })}
        />
        <Select
          hideLabel
          label={t("settings.translation.profile")}
          value={profile?.id ?? ""}
          disabled={disabled || switching || !profiles.length}
          options={profiles.map((item) => ({ value: item.id, label: profileLabel(item) }))}
          onChange={(profileId) => void selectProfile(profileId)}
        />
        {usesModels && (
          <div className="translation-route-model">
            <EditableDropdownField
              label={t("settings.translation.model")}
              value={target.model}
              options={models.map((model) => ({ value: model, label: model }))}
              disabled={disabled || switching}
              optionsDisabled={loading || !models.length}
              placeholder={t("settings.translation.manualModel")}
              onChange={(model) => onChange({ ...target, model })}
            />
            <button
              className="translation-route-icon-button"
              type="button"
              aria-label={t("common.refresh")}
              disabled={disabled || loading}
              onClick={() => void refresh()}
            >
              <RefreshCw size={14} className={loading ? "spin" : undefined} />
            </button>
          </div>
        )}
        {error && <small className="api-model-catalog-error">{error}</small>}
        {thinkingControl === "disable_supported" && (
          <PreferenceToggle
            title={t("settings.translation.thinkingMode")}
            checked={target.thinking_enabled}
            disabled={disabled}
            onChange={(thinking_enabled) => onChange({ ...target, thinking_enabled })}
          />
        )}
      </div>
      <div className="translation-route-actions">
        <button type="button" aria-label={t("settings.translation.moveUp")} disabled={disabled || index === 0} onClick={onMoveUp}><ArrowUp size={14} /></button>
        <button type="button" aria-label={t("settings.translation.moveDown")} disabled={disabled || !canMoveDown} onClick={onMoveDown}><ArrowDown size={14} /></button>
        <button type="button" aria-label={t("common.delete")} disabled={disabled || !canDelete} onClick={onDelete}><Trash2 size={14} /></button>
      </div>
    </div>
  );
}
