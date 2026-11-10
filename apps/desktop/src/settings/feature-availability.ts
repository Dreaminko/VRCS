import type { SettingsCategory } from "./settings-types";
import type { FeatureSettings, Settings } from "./types";
import type { CoreCapabilities } from "../core-client/capabilities";

export const DEFAULT_FEATURE_SETTINGS: FeatureSettings = {
  glossary: true, learning: true, anki: true, microphone_speaker_filter: false,
  vrcx: true, ocr: true, vr_overlay: true, external_api: true, visual_question_answering: false,
};

export function visibleSettingsCategories(features: FeatureSettings): SettingsCategory[] {
  const categories: SettingsCategory[] = ["system", "audio", "recognition", "translation"];
  if (features.vr_overlay) categories.push("vr_overlay");
  if (features.ocr || features.visual_question_answering) categories.push("vision");
  if (features.glossary) categories.push("glossary");
  categories.push("api");
  if (features.learning) categories.push("learning");
  categories.push("connections", "about");
  return categories;
}

export function resolveSettingsCategory(requested: SettingsCategory, features: FeatureSettings): SettingsCategory {
  const category = requested === "ocr" ? "vision" : requested;
  return visibleSettingsCategories(features).includes(category) ? category : "system";
}

export function captureUnavailableReason(settings: Settings | null, capabilities: CoreCapabilities | null): string | null {
  if (!settings || !capabilities) return "common.checking";
  const output = settings.audio.output.mode;
  const required = [
    output === "system" ? capabilities.audio.system_output : undefined,
    output === "vrchat" ? capabilities.audio.application_output : undefined,
    output === "vrchat_players" ? capabilities.audio.player_audio : undefined,
    settings.audio.microphone.mode !== "disabled" ? capabilities.audio.microphone : undefined,
    settings.vrchat_chatbox.enabled ? capabilities.chatbox.network_receive : undefined,
  ];
  const unavailable = required.find((capability) => capability && !capability.available);
  if (unavailable) return unavailable.reason_code ?? "platform.capture_unavailable";
  if (settings.asr.backend === "qwen_local_managed" && !capabilities.inference.qwen_cpu.supported) return "platform.qwen_unavailable";
  return null;
}
