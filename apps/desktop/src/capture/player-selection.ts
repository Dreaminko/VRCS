import type { Settings } from "../settings/types";
import type { SpeakerIdentity } from "../subtitles/types";

export function playerUserId(speaker: SpeakerIdentity): string | null {
  const id = speaker.user_id;
  return id && /^usr_[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(id) ? id : null;
}

/** Refresh labels only for selected accounts; names never determine selection. */
export function withCurrentPlayerNames(settings: Settings, speakers: readonly SpeakerIdentity[]): Settings {
  const options = settings.audio.output.player_audio;
  if (!options?.selected_tracks?.length) return settings;
  const selected = new Set(options.selected_tracks);
  let names = options.selected_user_names ?? {};
  let changed = false;
  for (const speaker of speakers) {
    const id = playerUserId(speaker);
    const name = speaker.display_name;
    if (!id || !selected.has(id) || !name || [...name].length > 128 || /[\p{Cc}]/u.test(name) || names[id] === name) continue;
    if (!changed) names = { ...names };
    names[id] = name;
    changed = true;
  }
  return changed ? { ...settings, audio: { ...settings.audio, output: { ...settings.audio.output,
    player_audio: { ...options, selected_user_names: names },
  } } } : settings;
}
