import { Globe } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { RoomContextStatus } from "../../core-client/types";
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
  status,
  applySettings,
}: {
  draft: Settings;
  status?: RoomContextStatus | null;
  applySettings: ApplySettings;
}) {
  const { t } = useTranslation();
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

  const statusText = status
    ? status.availability === "unavailable"
      ? t("settings.capture.roomContext.statusUnavailable")
      : t(`settings.capture.roomContext.tier.${status.tier}`, {
          world: status.world_name ?? t("settings.capture.roomContext.worldUnknown"),
          count: status.member_count,
        })
    : null;

  return (
    <section className="capture-chatbox-settings" aria-labelledby="vrchat-room-context-title">
      <div className="device-group-heading">
        <div className="device-group-title">
          <span className="device-group-icon"><Globe size={18} /></span>
          <div>
            <h3 id="vrchat-room-context-title">{t("settings.capture.roomContext.title")}</h3>
            <p className="settings-field-note">{t("settings.capture.roomContext.description")}</p>
          </div>
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
        {statusText && (
          <p className="settings-field-note" role="status" aria-live="polite">
            {statusText}
          </p>
        )}
      </div>
    </section>
  );
}
