import { CircleQuestionMark, Globe } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { ApplySettings } from "../settings-types";
import { PreferenceToggle } from "../SettingsControls";
import type { Settings, VrchatContextConfig } from "../types";

export const DEFAULT_VRCHAT_CONTEXT_CONFIG: VrchatContextConfig = {
  include_in_asr_context: false,
  include_in_translation_context: false,
  include_world_name: true,
};

export function VrchatRoomContextSettings({
  draft,
  applySettings,
}: {
  draft: Settings;
  applySettings: ApplySettings;
}) {
  const { t } = useTranslation();
  const title = t("settings.capture.roomContext.title");
  const titleTailMatch = title.match(/^(.*?)(\S+)$/);
  const titleLeading = titleTailMatch && titleTailMatch[1] ? titleTailMatch[1] : title.slice(0, -1);
  const titleTrailing = titleTailMatch && titleTailMatch[1] ? titleTailMatch[2] : title.slice(-1);
  const description = t("settings.capture.roomContext.description");
  const value = {
    ...DEFAULT_VRCHAT_CONTEXT_CONFIG,
    ...draft.vrchat_context,
  };
  const update = (patch: Partial<VrchatContextConfig>) =>
    applySettings((current) => ({
      ...current,
      vrchat_context: {
        ...DEFAULT_VRCHAT_CONTEXT_CONFIG,
        ...current.vrchat_context,
        ...patch,
      },
    }));

  return (
    <section className="capture-chatbox-settings capture-room-context-settings" aria-labelledby="vrchat-room-context-title">
      <div className="device-group-title">
        <span className="device-group-icon"><Globe size={18} /></span>
        <div className="device-group-title-heading">
          <h3 id="vrchat-room-context-title" aria-label={title}>
            {titleLeading}
            <span className="device-group-title-tail">
              <span className="device-group-title-tail-text">{titleTrailing}</span>
              <button
                type="button"
                className="settings-help-trigger"
                aria-label={title}
                aria-description={description}
                data-tooltip={description}
                data-tooltip-prose=""
              >
                <CircleQuestionMark size={15} strokeWidth={1.9} aria-hidden="true" />
              </button>
            </span>
          </h3>
        </div>
      </div>
      <div className="device-group-content">
        <PreferenceToggle
          title={t("settings.capture.roomContext.asr")}
          description={t("settings.capture.roomContext.asrDescription")}
          checked={value.include_in_asr_context}
          disabled={false}
          onChange={(include_in_asr_context) => update({ include_in_asr_context })}
        />
        <PreferenceToggle
          title={t("settings.capture.roomContext.translation")}
          description={t("settings.capture.roomContext.translationDescription")}
          checked={value.include_in_translation_context}
          disabled={false}
          onChange={(include_in_translation_context) =>
            update({ include_in_translation_context })
          }
        />
        <PreferenceToggle
          title={t("settings.capture.roomContext.worldName")}
          description={t("settings.capture.roomContext.worldNameDescription")}
          checked={value.include_world_name}
          disabled={!value.include_in_asr_context && !value.include_in_translation_context}
          onChange={(include_world_name) => update({ include_world_name })}
        />
      </div>
    </section>
  );
}
