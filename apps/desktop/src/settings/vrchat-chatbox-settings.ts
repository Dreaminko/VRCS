import type { ApiProfileView } from "../providers/types";
import { translationSelectionReady } from "./model-selection.ts";
import type { TranslationSettings, VrchatChatboxConfig } from "./types";

export const DEFAULT_VRCHAT_CHATBOX_CONFIG: VrchatChatboxConfig = { enabled: false };

export function chatboxTranslationReady(translation: TranslationSettings, profiles: ApiProfileView[]): boolean {
  return translation.mode === "automatic"
    && translation.speaker_targets.some((target) => translationSelectionReady(target, profiles));
}
