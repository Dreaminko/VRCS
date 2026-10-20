import { ArrowDown, ArrowUp, Plus, RefreshCw, Trash2 } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { localizedError } from "../../app/app-utils";
import { providersApi } from "../../providers/api";
import { liveTranslationServiceName } from "../../providers/recognition-services";
import { supportsLlmModels, supportsTranslation } from "../../providers/api-profile-purpose";
import { EditableDropdownField } from "../../shared/ui/DropdownField";
import { LanguagePicker } from "../../shared/ui/LanguagePicker";
import { TRANSLATION_LANGUAGE_CODES } from "../../translation/translation-languages";
import { selectTranslationModel, switchTranslationProfile, translationModels, updateTranslationModel } from "../../translation/translation-model-selection";
import { normalizeReasoningControlForModel } from "../../translation/translation-thinking";
import type { ApiProfileView } from "../../providers/types";
import type { TranslationTargetSettings } from "../types";
import { useProfileModels } from "../hooks/useProfileModels";
import { Select } from "../SettingsControls";
import { ReasoningControlField } from "../ReasoningControlField";
import { targetLanguageCodes } from "./language-settings";

function profileLabel(profile: ApiProfileView): string {
  return profile.name === profile.provider_display_name
    ? profile.name
    : `${profile.name} · ${profile.provider_display_name}`;
}

export function TranslationRouteList({
  title,
  liveService,
  targets,
  profiles,
  disabled,
  onChange,
}: {
  title: string;
  liveService?: string;
  targets: TranslationTargetSettings[];
  profiles: ApiProfileView[];
  disabled: boolean;
  onChange: (targets: TranslationTargetSettings[]) => void;
}) {
  const { t } = useTranslation();
  const [adding, setAdding] = useState(false);
  const [addError, setAddError] = useState("");
  const translationProfiles = profiles.filter(supportsTranslation);
  const addTarget = async () => {
    if (adding || targets.length >= 3) return;
    const profile = translationProfiles.find((item) => item.id === targets[0]?.profile_id) ?? translationProfiles[0];
    if (!profile) return;
    const used = new Set(targets.map((target) => target.target_language));
    const targetLanguage = (profile.capabilities.supported_languages.length
      ? profile.capabilities.supported_languages
      : TRANSLATION_LANGUAGE_CODES
    ).find((code) => !used.has(code)) ?? "en";
    setAdding(true);
    setAddError("");
    try {
      let model = targets.find((target) => target.profile_id === profile.id && target.model.trim())?.model ?? "";
      if (supportsLlmModels(profile) && !model) {
        const catalog = await providersApi.apiProfileModels(profile.id);
        model = selectTranslationModel(profile.provider, catalog.models, "", catalog.model_details) ?? "";
        if (!model) {
          setAddError(t("settings.translation.manualModel"));
          return;
        }
      }
      onChange([
        ...targets,
        {
          target_language: targetLanguage,
          profile_id: profile.id,
          model,
          thinking_enabled: false,
        },
      ]);
    } catch (reason) {
      setAddError(localizedError(reason, t, "errors.apiProfiles.models"));
    } finally {
      setAdding(false);
    }
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
        </div>
      </header>
      <div className="translation-route-list">
        {targets.map((target, index) => (
          <TranslationRouteRow
            key={`${target.target_language}-${index}`}
            index={index}
            liveService={index === 0 ? liveService : undefined}
            target={target}
            profiles={translationProfiles}
            usedLanguages={targets.map((item) => item.target_language)}
            disabled={disabled || adding}
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
        disabled={disabled || adding || targets.length >= 3 || !translationProfiles.length}
        onClick={() => void addTarget()}
      >
        <Plus size={14} />
        {t("settings.translation.addRoute")}
      </button>
      {addError && <small className="api-model-catalog-error" role="alert">{addError}</small>}
    </section>
  );
}

function TranslationRouteRow({
  index,
  liveService,
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
  liveService?: string;
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
  const native = liveTranslationServiceName(liveService);
  const [switching, setSwitching] = useState(false);
  const profile = profiles.find((item) => item.id === target.profile_id);
  const usesModels = Boolean(!native && profile && supportsLlmModels(profile));
  const { models, modelDetails, loading, error, refresh, load } = useProfileModels(profile, usesModels);
  const languageCodes = targetLanguageCodes(target, usedLanguages, profiles, liveService);
  const [reasoningReset, setReasoningReset] = useState(false);
  const selectProfile = async (profileId: string) => {
    const nextProfile = profiles.find((item) => item.id === profileId);
    if (!nextProfile) return;
    setSwitching(true);
    try {
      const remembered = Object.hasOwn(target.model_by_profile ?? {}, profileId);
      const available = !remembered && supportsLlmModels(nextProfile) && nextProfile.provider !== "openai_compatible"
        ? await load(nextProfile.id)
        : { models: [], model_details: {} };
      const next = switchTranslationProfile(target, nextProfile, available.models, available.model_details);
      const reasoning_control = normalizeReasoningControlForModel(nextProfile.provider, next.model, target.reasoning_control, available.model_details?.[next.model]);
      setReasoningReset(Boolean(target.reasoning_control && !reasoning_control));
      onChange({ ...next, reasoning_control });
    } finally {
      setSwitching(false);
    }
  };

  return (
    <div className="translation-route-row">
      <div className="translation-route-rank">
        {index + 1}
      </div>
      <div className="translation-route-fields">
        <LanguagePicker
          compact
          label={t("settings.translation.targetLanguageSettings")}
          value={target.target_language}
          languageCodes={languageCodes}
          allowCustom={!native && (profile?.capabilities.supports_custom_translation_language ?? false)}
          disabled={disabled}
          onChange={(target_language) => onChange({ ...target, target_language })}
        />
        {native ? <small>{native}</small> : <Select
          hideLabel
          label={t("settings.translation.profile")}
          value={profile?.id ?? ""}
          disabled={disabled || switching || !profiles.length}
          options={profiles.map((item) => ({ value: item.id, label: profileLabel(item) }))}
          onChange={(profileId) => void selectProfile(profileId)}
        />}
        {usesModels && (
          <div className="translation-route-model">
            <EditableDropdownField
              commitOnBlur
              label={t("settings.translation.model")}
              value={target.model}
              options={translationModels(models, modelDetails).map((model) => ({ value: model, label: model }))}
              disabled={disabled || switching}
              optionsDisabled={loading || !models.length}
              placeholder={t("settings.translation.manualModel")}
              onChange={(model) => {
                const reasoning_control = normalizeReasoningControlForModel(profile?.provider, model, target.reasoning_control, modelDetails[model]);
                setReasoningReset(Boolean(target.reasoning_control && !reasoning_control));
                onChange({ ...updateTranslationModel(target, model), reasoning_control });
              }}
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
        {!native && usesModels && <ReasoningControlField provider={profile?.provider} model={target.model}
          details={modelDetails[target.model]} thinkingEnabled={target.thinking_enabled} value={target.reasoning_control}
          disabled={disabled} onChange={(patch) => { setReasoningReset(false); onChange({ ...target, ...patch }); }} />}
        {reasoningReset && <small>{t("settings.reasoning.reset")}</small>}
      </div>
      <div className="translation-route-actions">
        <button type="button" aria-label={t("settings.translation.moveUp")} disabled={disabled || index === 0} onClick={onMoveUp}><ArrowUp size={14} /></button>
        <button type="button" aria-label={t("settings.translation.moveDown")} disabled={disabled || !canMoveDown} onClick={onMoveDown}><ArrowDown size={14} /></button>
        <button type="button" aria-label={t("common.delete")} disabled={disabled || !canDelete} onClick={onDelete}><Trash2 size={14} /></button>
      </div>
    </div>
  );
}
