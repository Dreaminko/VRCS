import type { Settings } from "../settings/types";
import type { SpeakerIdentity, Subtitle } from "./types";
import type { PlayerAudioSettings, PlayerSourceMode } from "../capture/types";
import { isPlayerUserId, playerUserId } from "../capture/player-selection.ts";
import { historySpeakers, isOtherPlayerSource, sourceLanguageKey, subtitleSourceLanguage, type PlayerHistoryPreferences } from "./player-history.ts";
import { TRANSLATION_LANGUAGE_CODES } from "../translation/translation-languages.ts";

export type PlayerVisibilityAction =
  | { set_track_hidden: { track_id: string; hidden: boolean } }
  | { show_all_tracks: null }
  | { set_follow_default: boolean }
  | { set_source_language_hidden: { language: string; hidden: boolean } }
  | { clear_hidden_languages: null };
export type PlayerProximityAction = { set_proximity_radius: number };
export type PlayerViewPriorityAction = { set_view_priority: { enabled: boolean; horizontal_fov_deg: number } };
export type PlayerSourceAction =
  | { set_user_source_disabled: { user_id: string; source: "vrchat_chatbox" | "speaker"; disabled: boolean } };
export type PlayerSourceModeAction = { set_source_mode: PlayerSourceMode };
export type PlayerDisplayAction = PlayerVisibilityAction | PlayerProximityAction | PlayerViewPriorityAction | PlayerSourceAction | PlayerSourceModeAction;

export function isPlayerProximityAction(action: PlayerDisplayAction): action is PlayerProximityAction {
  return "set_proximity_radius" in action;
}

export function isPlayerViewPriorityAction(action: PlayerDisplayAction): action is PlayerViewPriorityAction {
  return "set_view_priority" in action;
}

export function isPlayerSourceAction(action: PlayerDisplayAction): action is PlayerSourceAction {
  return "set_user_source_disabled" in action;
}

export function isPlayerSourceModeAction(action: PlayerDisplayAction): action is PlayerSourceModeAction {
  return "set_source_mode" in action;
}

export function collectPlayerSpeakers(subtitles: readonly Subtitle[], current: readonly SpeakerIdentity[], selected: readonly string[] | null, names: Record<string, string>): SpeakerIdentity[] {
  const known = new Map(historySpeakers(subtitles).map(speaker => [speaker.id, speaker]));
  for (const speaker of current) known.set(speaker.id, { ...speaker, display_name: speaker.display_name || known.get(speaker.id)?.display_name });
  const currentIds = new Set(current.map(speaker => speaker.id));
  const saved = new Set((selected ?? []).filter(isPlayerUserId));
  const kept = new Map<string, SpeakerIdentity>();
  const accounts = new Map<string, string>();
  for (const [track_id, speaker] of known) {
    if (currentIds.has(track_id)) {
      kept.set(track_id, speaker);
      continue;
    }
    // History keeps one row per account that has a captured user ID and a
    // saved source setting; records without either cannot be restored here.
    const user_id = playerUserId(speaker);
    if (!user_id || !saved.has(user_id)) continue;
    const first = accounts.get(user_id);
    if (first === undefined) {
      accounts.set(user_id, track_id);
      kept.set(track_id, speaker);
    } else if (!kept.get(first)?.display_name && speaker.display_name) {
      kept.set(first, speaker);
    }
  }
  const present = new Set([...current.map(playerUserId), ...accounts.keys()].filter((id): id is string => id !== null));
  for (const user_id of saved) if (!present.has(user_id)) {
    kept.set(`saved-user:${user_id}`, { id: `saved-user:${user_id}`, user_id, index: 0, display_name: names[user_id] });
  }
  return [...kept.values()];
}

export function sortPlayerSpeakersByDistance(speakers: readonly SpeakerIdentity[], distances: ReadonlyMap<string, number | undefined>): SpeakerIdentity[] {
  return [...speakers].sort((left, right) => {
    const leftDistance = distances.get(left.id);
    const rightDistance = distances.get(right.id);
    if (leftDistance === undefined || !Number.isFinite(leftDistance)) return rightDistance === undefined || !Number.isFinite(rightDistance) ? 0 : 1;
    if (rightDistance === undefined || !Number.isFinite(rightDistance)) return -1;
    return leftDistance - rightDistance;
  });
}

export function playerLanguageOptions(subtitles: readonly Subtitle[], defaults: readonly string[], hidden: readonly string[] | null): string[] {
  return [...new Set([...defaults, ...(hidden ?? []),
    ...subtitles.filter(s => isOtherPlayerSource(s.source)).map(subtitleSourceLanguage),
    ...TRANSLATION_LANGUAGE_CODES].map(sourceLanguageKey).filter((code): code is string => code !== null))];
}

function setMembership(values: readonly string[], key: string, included: boolean): string[] {
  return included ? [...new Set([...values, key])] : values.filter(value => value !== key);
}

function playerAudioOptions(options: PlayerAudioSettings | undefined): PlayerAudioSettings {
  return options ?? {
    pagefile_drive: "C:",
    opus_library_path: "",
    max_players: 0,
    max_visible_players: 4,
    proximity: { enabled: false, radius_m: 10 },
    view_priority: { enabled: false, horizontal_fov_deg: 120 },
    source_mode: "allowlist",
  };
}

export function sourceMode(options: PlayerAudioSettings | undefined): PlayerSourceMode {
  return options?.source_mode === "everyone" ? "everyone" : "allowlist";
}

/** A zero radius switches distance filtering off; any positive radius limits processing to that range. */
export function applyPlayerProximity(settings: Settings, radius_m: number): Settings {
  const options = playerAudioOptions(settings.audio.output.player_audio);
  return { ...settings, audio: { ...settings.audio, output: { ...settings.audio.output,
    player_audio: { ...options, proximity: { enabled: radius_m > 0, radius_m } },
  } } };
}

export function applyPlayerViewPriority(settings: Settings, enabled: boolean, horizontal_fov_deg: number): Settings {
  const options = playerAudioOptions(settings.audio.output.player_audio);
  const fov = Math.min(180, Math.max(30, horizontal_fov_deg));
  return { ...settings, audio: { ...settings.audio, output: { ...settings.audio.output,
    player_audio: { ...options, view_priority: { enabled, horizontal_fov_deg: fov } },
  } } };
}

export function applyPlayerSourceMode(settings: Settings, mode: PlayerSourceMode): Settings {
  const options = playerAudioOptions(settings.audio.output.player_audio);
  return { ...settings, audio: { ...settings.audio, output: { ...settings.audio.output,
    player_audio: { ...options, source_mode: mode },
  } } };
}

export function applyPlayerSourceAction(settings: Settings, action: PlayerSourceAction, speaker?: SpeakerIdentity): Settings {
  const options = playerAudioOptions(settings.audio.output.player_audio);
  const { user_id, source, disabled } = action.set_user_source_disabled;
  if (!isPlayerUserId(user_id)) throw new Error("Player identity is unavailable");
  const overrides = { ...(options.source_overrides ?? {}) };
  const previous = overrides[user_id];
  const mode = sourceMode(options);
  // A missing rule inherits the mode: off until enabled, or on until disabled.
  const unmatched = mode === "allowlist";
  const rule = {
    chatbox_disabled: source === "vrchat_chatbox" ? disabled : previous?.chatbox_disabled ?? unmatched,
    voice_disabled: source === "speaker" ? disabled : previous?.voice_disabled ?? unmatched,
  };
  // Store the entry only while it still differs from the mode's default.
  const keep = mode === "allowlist" ? !rule.chatbox_disabled || !rule.voice_disabled : rule.chatbox_disabled || rule.voice_disabled;
  if (keep) overrides[user_id] = rule;
  else delete overrides[user_id];
  if (Object.keys(overrides).length > 512) throw new Error("Save source settings for at most 512 players");
  // Labels only describe saved rules; stale entries would count toward the backend limit.
  const names = Object.fromEntries(Object.entries(options.selected_user_names ?? {}).filter(([id]) => id in overrides));
  const name = speaker?.display_name;
  if (keep && playerUserId(speaker ?? { id: "", index: 0 }) === user_id && name && [...name].length <= 128 && !/[\p{Cc}]/u.test(name)) names[user_id] = name;
  return { ...settings, audio: { ...settings.audio, output: { ...settings.audio.output,
    player_audio: { ...options, source_overrides: overrides, selected_user_names: names },
  } } };
}

export function applyPlayerVisibilityAction(preferences: PlayerHistoryPreferences, action: PlayerVisibilityAction, _defaults: readonly string[]): PlayerHistoryPreferences {
  if ("set_track_hidden" in action) return { ...preferences, hiddenTracks: setMembership(preferences.hiddenTracks, action.set_track_hidden.track_id, action.set_track_hidden.hidden) };
  if ("show_all_tracks" in action) return { ...preferences, hiddenTracks: [] };
  // Preserve the explicit list when an older VR client sends this action.
  if ("set_follow_default" in action) return { ...preferences, hiddenSourceLanguages: preferences.hiddenSourceLanguages ?? [] };
  if ("clear_hidden_languages" in action) return { ...preferences, hiddenSourceLanguages: [] };
  const language = sourceLanguageKey(action.set_source_language_hidden.language);
  if (!language) throw new Error("Invalid source language");
  return { ...preferences, hiddenSourceLanguages: setMembership(preferences.hiddenSourceLanguages ?? [], language, action.set_source_language_hidden.hidden) };
}
