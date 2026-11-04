import { supportsTranslation } from "../providers/api-profile-purpose.ts";
import type { ApiProfileView } from "../providers/types";
import type { TranslationSettings, VrchatChatboxConfig } from "./types";

export const DEFAULT_VRCHAT_CHATBOX_CONFIG: VrchatChatboxConfig = { enabled: false };

export function chatboxTranslationReady(translation: TranslationSettings, profiles: ApiProfileView[]): boolean {
  return translation.mode === "automatic" && translation.speaker_targets.some((target) => (
    profiles.some((profile) => profile.id === target.profile_id && supportsTranslation(profile))
  ));
}
