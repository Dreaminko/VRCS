import type { Settings } from "../settings/types";
import type { SpeakerIdentity, Subtitle } from "./types";
import { playerUserId } from "../capture/player-selection.ts";
import { historySpeakers, sourceLanguageKey, subtitleSourceLanguage, type PlayerHistoryPreferences } from "./player-history.ts";
import { TRANSLATION_LANGUAGE_CODES } from "../translation/translation-languages.ts";
import type { MicrophoneSpeakerAction } from "../capture/microphone-speakers";

export type PlayerSelectionAction =
  | { set_processing_mode: "all" | "selected" }
  | { set_user_selected: { user_id: string; selected: boolean } }
  | { select_current: null }
  | { clear_selection: null };
export type PlayerVisibilityAction =
  | { set_track_hidden: { track_id: string; hidden: boolean } }
  | { show_all_tracks: null }
  | { set_follow_default: boolean }
  | { set_source_language_hidden: { language: string; hidden: boolean } }
  | { clear_hidden_languages: null };
export type PlayerDisplayAction = PlayerSelectionAction | PlayerVisibilityAction | MicrophoneSpeakerAction;

export function isPlayerSelectionAction(action: PlayerDisplayAction): action is PlayerSelectionAction {
  return "set_processing_mode" in action || "set_user_selected" in action || "select_current" in action || "clear_selection" in action;
}

export function collectPlayerSpeakers(subtitles: readonly Subtitle[], current: readonly SpeakerIdentity[], selected: readonly string[] | null, names: Record<string, string>): SpeakerIdentity[] {
  const known = new Map(historySpeakers(subtitles).map(speaker => [speaker.id, speaker]));
  for (const speaker of current) known.set(speaker.id, { ...speaker, display_name: speaker.display_name || known.get(speaker.id)?.display_name });
  const users = new Set([...known.values()].map(playerUserId));
  for (const user_id of selected ?? []) if (!users.has(user_id)) {
    known.set(`saved-user:${user_id}`, { id: `saved-user:${user_id}`, user_id, index: 0, display_name: names[user_id] });
  }
  return [...known.values()];
}

export function playerLanguageOptions(subtitles: readonly Subtitle[], defaults: readonly string[], hidden: readonly string[] | null): string[] {
  return [...new Set([...defaults, ...(hidden ?? []),
    ...subtitles.filter(s => (s.source ?? "speaker") === "speaker").map(subtitleSourceLanguage),
    ...TRANSLATION_LANGUAGE_CODES].map(sourceLanguageKey).filter((code): code is string => code !== null))];
}

function setMembership(values: readonly string[], key: string, included: boolean): string[] {
  return included ? [...new Set([...values, key])] : values.filter(value => value !== key);
}

export function applyPlayerSelectionAction(settings: Settings, action: PlayerSelectionAction, speakers: readonly SpeakerIdentity[], current: readonly SpeakerIdentity[]): Settings {
  if (settings.audio.output.mode !== "vrchat_players") throw new Error("Player processing is unavailable");
  const options = settings.audio.output.player_audio ?? { pagefile_drive: "C:", opus_library_path: "", max_players: 0, max_visible_players: 4 };
  let selected = options.selected_tracks ?? null;
  if ("set_processing_mode" in action) {
    if (action.set_processing_mode === "all") selected = null;
    else selected = [];
  } else {
    if (selected === null) throw new Error("Player selection mode changed; try again");
    if ("clear_selection" in action) selected = [];
    else if ("select_current" in action) selected = [...new Set([...selected, ...current.map(playerUserId).filter((id): id is string => id !== null)])];
    else {
      const { user_id, selected: included } = action.set_user_selected;
      if (!speakers.some(s => playerUserId(s) === user_id) && !selected.includes(user_id)) throw new Error("Player identity is unavailable");
      selected = setMembership(selected, user_id, included);
    }
  }
  if ((selected?.length ?? 0) > 512) throw new Error("Select at most 512 players");
  const names = new Map(Object.entries(options.selected_user_names ?? {}));
  const validName = (name: string | null | undefined): name is string => !!name && [...name].length <= 128 && !/[\p{Cc}]/u.test(name);
  for (const speaker of speakers) {
    const id = playerUserId(speaker);
    if (id && validName(speaker.display_name) && !names.has(id)) names.set(id, speaker.display_name);
  }
  for (const speaker of current) {
    const id = playerUserId(speaker);
    if (id && validName(speaker.display_name)) names.set(id, speaker.display_name);
  }
  const selected_user_names = Object.fromEntries((selected ?? []).flatMap(id => names.has(id) ? [[id, names.get(id)!]] : []));
  return { ...settings, audio: { ...settings.audio, output: { ...settings.audio.output,
    player_audio: { ...options, selected_tracks: selected, selected_user_names },
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
