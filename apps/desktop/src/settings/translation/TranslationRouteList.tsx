import { ArrowDown, ArrowUp, Plus, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { liveTranslationServiceName } from "../../providers/recognition-services";
import type { ApiProfileView } from "../../providers/types";
import { LanguagePicker } from "../../shared/ui/LanguagePicker";
import { TRANSLATION_LANGUAGE_CODES } from "../../translation/translation-languages";
import { EMPTY_MODEL_BINDING, resolveModelSelection } from "../model-selection";
import type { ModelBinding, TranslationTargetSettings } from "../types";
import { targetLanguageCodes } from "./language-settings";

export function TranslationRouteList({ title, liveService, targets, profiles, modelSelection = EMPTY_MODEL_BINDING, disabled, onChange }: {
  title: string;
  liveService?: string;
  targets: TranslationTargetSettings[];
  profiles: ApiProfileView[];
  modelSelection?: ModelBinding;
  disabled: boolean;
  onChange: (targets: TranslationTargetSettings[]) => void;
}) {
  const { t } = useTranslation();
  const addTarget = () => {
    if (targets.length >= 3) return;
    const used = new Set(targets.map((target) => target.target_language));
    const profile = profiles.find((item) => item.id === modelSelection.profile_id);
    const languages = profile?.capabilities.supported_languages.length
      ? profile.capabilities.supported_languages : TRANSLATION_LANGUAGE_CODES;
    const target_language = languages.find((code) => !used.has(code)) ?? "en";
    onChange([...targets, { target_language, ...modelSelection, use_default_model: true, thinking_enabled: false }]);
  };
  const move = (index: number, offset: number) => {
    const next = [...targets];
    [next[index], next[index + offset]] = [next[index + offset], next[index]];
    onChange(next);
  };
  return <section className="translation-route-group">
    <header className="translation-route-group-header"><div><strong>{title}</strong></div></header>
    <div className="translation-route-list">
      {targets.map((target, index) => {
        const service = index === 0 ? liveService : undefined;
        const native = liveTranslationServiceName(service);
        const resolved = { ...target, ...resolveModelSelection(target, modelSelection) };
        const profile = profiles.find((item) => item.id === resolved.profile_id);
        return <div className="translation-route-row" key={`${target.target_language}-${index}`}>
          <div className="translation-route-rank">{index + 1}</div>
          <LanguagePicker compact label={t("settings.translation.targetLanguageSettings")} value={target.target_language}
            languageCodes={targetLanguageCodes(resolved, targets.map((item) => item.target_language), profiles, service)}
            allowCustom={!native && (profile?.capabilities.supports_custom_translation_language ?? false)} disabled={disabled}
            onChange={(target_language) => onChange(targets.map((item, i) => i === index ? { ...item, target_language } : item))} />
          <div className="translation-route-actions">
            <button type="button" aria-label={t("settings.translation.moveUp")} data-tooltip={t("settings.translation.moveUp")}
              disabled={disabled || index === 0} onClick={() => move(index, -1)}><ArrowUp size={14} /></button>
            <button type="button" aria-label={t("settings.translation.moveDown")} data-tooltip={t("settings.translation.moveDown")}
              disabled={disabled || index === targets.length - 1} onClick={() => move(index, 1)}><ArrowDown size={14} /></button>
            <button type="button" aria-label={t("common.delete")} data-tooltip={t("common.delete")}
              disabled={disabled || targets.length <= 1} onClick={() => onChange(targets.filter((_, i) => i !== index))}><Trash2 size={14} /></button>
          </div>
        </div>;
      })}
    </div>
    <button className="secondary-button translation-route-add" type="button" disabled={disabled || targets.length >= 3}
      onClick={addTarget}><Plus size={14} />{t("settings.translation.addRoute")}</button>
  </section>;
}
