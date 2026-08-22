import { Languages, Workflow } from "lucide-react";
import { useTranslation } from "react-i18next";

import { supportsContext, supportsTranslation } from "../../api-profile-purpose";
import type { ApiProfileView } from "../../providers/types";
import type {
  Settings,
  TranslationSettings,
} from "../types";
import type { ApplySettings, SaveState } from "../settings-types";
import { Select } from "../SettingsControls";
import { LanguagePresetSettings } from "../translation/LanguagePresetSettings";
import { TranslationEnhancementSettings } from "../translation/TranslationEnhancementSettings";
import { TranslationRouteList } from "../translation/TranslationRouteList";

export function TranslationSettingsSection({ draft, apiProfiles, saveState, applySettings }: {
  draft: Settings;
  apiProfiles: ApiProfileView[];
  saveState: SaveState;
  applySettings: ApplySettings;
}) {
  const { t } = useTranslation();
  const translationProfiles = apiProfiles.filter(supportsTranslation);
  const preferred = draft.translation.speaker_targets[0];
  const enhancementProfile = translationProfiles.find(
    (profile) => profile.id === preferred?.profile_id,
  );
  const controlsDisabled = saveState === "saving";
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
        <div className="translation-config-row">
          <div className="translation-config-title">
            <Workflow size={17} />
            <span><strong>{t("settings.translation.mode")}</strong></span>
          </div>
          <div className="translation-config-fields translation-config-fields-single">
            <Select
              label={t("settings.translation.mode")}
              value={draft.translation.mode}
              disabled={controlsDisabled || !translationProfiles.length}
              helper={!translationProfiles.length ? t("settings.translation.noProfiles") : undefined}
              options={["disabled", "manual", "automatic"].map((value) => ({
                value,
                label: t(`settings.translation.modes.${value}`),
              }))}
              onChange={(mode) => updateTranslation({
                ...draft.translation,
                mode: mode as TranslationSettings["mode"],
              })}
            />
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
              profiles={translationProfiles}
              disabled={controlsDisabled}
              onChange={(microphone_targets) => updateTranslation({
                ...draft.translation,
                microphone_targets,
              })}
            />
            <TranslationRouteList
              title={t("settings.translation.targetLanguageForOtherParty")}
              targets={draft.translation.speaker_targets}
              profiles={translationProfiles}
              disabled={controlsDisabled}
              onChange={(speaker_targets) => updateTranslation({
                ...draft.translation,
                speaker_targets,
              })}
            />
            <LanguagePresetSettings
              settings={draft}
              disabled={controlsDisabled}
              onChange={updateSettings}
            />
          </div>
        </div>

        {enhancementProfile && supportsContext(enhancementProfile) && (
          <TranslationEnhancementSettings
            translation={draft.translation}
            preferredTarget={preferred?.target_language ?? "zh-Hans"}
            profile={enhancementProfile}
            disabled={controlsDisabled}
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
