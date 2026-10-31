import { Languages, Workflow } from "lucide-react";
import { useTranslation } from "react-i18next";

import { liveTranslationServiceName } from "../../providers/recognition-services";
import { supportsContext, supportsLlmModels, supportsTranslation } from "../../providers/api-profile-purpose";
import type { ApiProfileView } from "../../providers/types";
import type {
  Settings,
  TranslationSettings,
} from "../types";
import type { ApplySettings } from "../settings-types";
import { LanguagePresetSettings } from "../translation/LanguagePresetSettings";
import { TranslationEnhancementSettings } from "../translation/TranslationEnhancementSettings";
import { TranslationRouteList } from "../translation/TranslationRouteList";

export function TranslationSettingsSection({ draft, apiProfiles, applySettings }: {
  draft: Settings;
  apiProfiles: ApiProfileView[];
  applySettings: ApplySettings;
}) {
  const { t } = useTranslation();
  const liveTranslate = Boolean(liveTranslationServiceName(draft.asr.backend));
  const translationProfiles = apiProfiles.filter(supportsTranslation);
  const translationUnavailable = !liveTranslate && !translationProfiles.length;
  const nativeFirstRoute = liveTranslate && draft.translation.mode === "automatic";
  const enhancementRoutes = [draft.translation.speaker_targets, draft.translation.microphone_targets]
    .flatMap((targets) => nativeFirstRoute ? targets.slice(1) : targets)
    .filter((target) => translationProfiles.some((profile) => profile.id === target.profile_id && supportsLlmModels(profile) && supportsContext(profile)));
  const localContextOnly = enhancementRoutes.every((target) => {
    const profile = translationProfiles.find((item) => item.id === target.profile_id);
    return profile?.is_local ?? profile?.capabilities.is_local ?? false;
  });
  const updateTranslation = (translation: TranslationSettings) => applySettings((current) => ({
    ...current,
    translation,
  }));
  const updateSettings = (settings: Settings) => applySettings(() => settings);

  return (
    <div className="settings-section settings-section-active translation-section" id="settings-panel-translation" role="tabpanel" aria-labelledby="settings-tab-translation">
      <div className="section-heading">
        <div>
          <Languages size={18} />
          <h2>{t("settings.translation.title")}</h2>
        </div>
      </div>

      <div className="translation-config">
        <div className="translation-config-row translation-mode-row">
          <div className="translation-config-title">
            <Workflow size={17} />
            <span><strong>{t("settings.translation.mode")}</strong></span>
          </div>
          <div className="field">
            <div className="theme-toggle-group translation-mode-options" role="group" aria-label={t("settings.translation.mode")}>
              {(["disabled", "manual", "automatic"] as const).map((mode) => (
                <button
                  key={mode}
                  type="button"
                  aria-pressed={draft.translation.mode === mode}
                  className={draft.translation.mode === mode ? "active" : ""}
                  disabled={translationUnavailable}
                  onClick={() => updateTranslation({ ...draft.translation, mode })}
                >
                  {t(`settings.translation.modes.${mode}`)}
                </button>
              ))}
            </div>
            {translationUnavailable && <small>{t("settings.translation.noProfiles")}</small>}
          </div>
        </div>

        <div className="translation-config-row translation-routes-row">
          <div className="translation-config-title">
            <Languages size={17} />
            <span>
              <strong>{t("settings.translation.targetLanguageSettings")}</strong>
            </span>
          </div>
          <div className="translation-route-groups">
            <TranslationRouteList
              title={t("settings.translation.targetLanguageForSelf")}
              targets={draft.translation.microphone_targets}
              liveService={liveTranslate && draft.translation.mode === "automatic" ? draft.asr.backend : undefined}
              profiles={translationProfiles}
              disabled={false}
              onChange={(microphone_targets) => updateTranslation({
                ...draft.translation,
                microphone_targets,
              })}
            />
            <TranslationRouteList
              title={t("settings.translation.targetLanguageForOtherParty")}
              targets={draft.translation.speaker_targets}
              liveService={liveTranslate && draft.translation.mode === "automatic" ? draft.asr.backend : undefined}
              profiles={translationProfiles}
              disabled={false}
              onChange={(speaker_targets) => updateTranslation({
                ...draft.translation,
                speaker_targets,
              })}
            />
            <LanguagePresetSettings
              settings={draft}
              disabled={false}
              onChange={updateSettings}
            />
          </div>
        </div>

        {enhancementRoutes.length > 0 && (
          <TranslationEnhancementSettings
            translation={draft.translation}
            preferredTarget={enhancementRoutes[0].target_language}
            localContextOnly={localContextOnly}
            disabled={false}
            onChange={(patch) => updateTranslation({
              ...draft.translation,
              prompt: { ...draft.translation.prompt, ...patch },
            })}
          />
        )}
      </div>
    </div>
  );
}
