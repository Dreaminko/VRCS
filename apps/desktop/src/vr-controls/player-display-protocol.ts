import type { Settings } from "../settings/types";
import type { SpeakerIdentity } from "../subtitles/types";
import { playerUserId } from "../capture/player-selection.ts";
import { sourceLanguageKey, type PlayerHistoryPreferences } from "../subtitles/player-history.ts";
import { applyPlayerProximity, applyPlayerSelectionAction, applyPlayerVisibilityAction, isPlayerProximityAction, isPlayerSelectionAction, type PlayerDisplayAction } from "../subtitles/player-controls.ts";
import type { ControlAction, ControlSource } from "./protocol";
import type { MicrophoneSpeakerAction, MicrophoneSpeakerSnapshot } from "../capture/microphone-speakers";

export type { PlayerDisplayAction } from "../subtitles/player-controls.ts";

export interface PlayerDisplayRow {
  track_id: string | null;
  user_id: string | null;
  name: string;
  detail: string;
  current: boolean;
  selected: boolean;
  hidden: boolean;
}

export interface PlayerDisplayView {
  available: boolean;
  processing_available: boolean;
  player_mode: boolean;
  selection_mode: "all" | "selected";
  selected_count: number;
  players: PlayerDisplayRow[];
  proximity?: {
    enabled: boolean;
    radius_m: number;
  };
  microphone?: Omit<MicrophoneSpeakerSnapshot, "snapshot_sequence" | "speakers"> & {
    speakers: Array<{ id: string; index: number; decision: "allow" | "block"; name: string; last_active: string }>;
  };
  languages: Array<{ code: string; label: string; hidden: boolean }>;
  follow_default: boolean;
  default_languages: string[];
  labels: Record<string, string>;
  busy: boolean;
  error: string | null;
}

export interface WristMenuUi {
  page: "home" | "players" | "languages" | "microphone";
  group: "current" | "history";
  player_page: number;
  language_page: number;
  microphone_page?: number;
}

export interface SliderCommand {
  action: "proximity_radius";
  min: number;
  max: number;
  step: number;
}

export type WristMenuCommand =
  | { control: ControlAction }
  | { page: WristMenuUi["page"] }
  | { group: WristMenuUi["group"] }
  | { player_page: number }
  | { language_page: number }
  | { microphone_page: number }
  | { slider: SliderCommand }
  | { display: PlayerDisplayAction };

export interface PlayerDisplayRequest {
  owner_epoch: number;
  request_id: number;
  base_revision: number;
  source: ControlSource;
  action: PlayerDisplayAction;
}

export function createPlayerDisplayPublisher(options: {
  getView: () => PlayerDisplayView;
  update: (revision: number, view: PlayerDisplayView) => Promise<void>;
}) {
  let revision = 0;
  let latest: { serialized: string; completion: Promise<void> } | null = null;
  return {
    revision: () => revision,
    publish: (): Promise<void> => {
      const view = options.getView();
      const serialized = JSON.stringify(view);
      if (latest?.serialized === serialized) return latest.completion;
      const next = { serialized, completion: Promise.resolve() };
      latest = next;
      next.completion = options.update(++revision, view).catch(reason => {
        if (latest === next) latest = null;
        throw reason;
      });
      return next.completion;
    },
  };
}

export function createPlayerDisplayReceiver(options: {
  epoch: number;
  active: () => boolean;
  execute: (action: PlayerDisplayAction) => Promise<void>;
  publish: () => Promise<void>;
  finish: (request: PlayerDisplayRequest, error: string | null) => Promise<void>;
}) {
  const seen = new Map<ControlSource, number>();
  return async (request: PlayerDisplayRequest): Promise<void> => {
    if (!options.active() || request.owner_epoch !== options.epoch
      || request.request_id <= (seen.get(request.source) ?? 0)) return;
    seen.set(request.source, request.request_id);
    let error: string | null = null;
    try {
      await options.execute(request.action);
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    }
    if (!options.active()) return;
    try {
      await options.publish();
    } catch (reason) {
      error ??= reason instanceof Error ? reason.message : String(reason);
    }
    if (options.active()) await options.finish(request, error);
  };
}

export interface PlayerDisplayExecutionState {
  available: boolean;
  processingAvailable: boolean;
  speakers: readonly SpeakerIdentity[];
  current: readonly SpeakerIdentity[];
  defaultLanguages: readonly string[];
  languages: readonly string[];
}

export function createPlayerDisplayExecutor(options: {
  getState: () => PlayerDisplayExecutionState;
  updateSettings: (apply: (settings: Settings) => Settings) => Promise<Settings>;
  updatePreferences: (apply: (preferences: PlayerHistoryPreferences) => PlayerHistoryPreferences) => void;
  updateMicrophone?: (action: MicrophoneSpeakerAction) => Promise<void>;
}) {
  const state = () => {
    const current = options.getState();
    if (!current.available) throw new Error("Player display is unavailable");
    return current;
  };
  return async (action: PlayerDisplayAction): Promise<void> => {
    state();
    if ("set_microphone_filter" in action || "set_microphone_speaker" in action) {
      if (!options.updateMicrophone) throw new Error("Microphone speaker controls are unavailable");
      await options.updateMicrophone(action);
      return;
    }
    if (isPlayerProximityAction(action)) {
      const saved = await options.updateSettings(settings => {
        if (settings.audio.output.mode !== "vrchat_players") throw new Error("Player processing is unavailable");
        const proximity = settings.audio.output.player_audio?.proximity;
        if (!proximity) throw new Error("Distance filtering is unavailable");
        if ("set_proximity_enabled" in action) {
          return proximity.enabled === action.set_proximity_enabled ? settings
            : applyPlayerProximity(settings, { ...proximity, enabled: action.set_proximity_enabled });
        }
        if (!Number.isFinite(action.set_proximity_radius)) throw new Error("Distance limit is invalid");
        const radius_m = Math.round(action.set_proximity_radius * 10) / 10;
        if (radius_m < 0 || radius_m > 10) throw new Error("Distance limit is invalid");
        return proximity.radius_m === radius_m ? settings : applyPlayerProximity(settings, { ...proximity, radius_m });
      });
      const proximity = saved.audio.output.player_audio?.proximity;
      const applied = "set_proximity_enabled" in action
        ? proximity?.enabled === action.set_proximity_enabled
        : proximity !== undefined && Math.abs(proximity.radius_m - Math.round(action.set_proximity_radius * 10) / 10) < 0.001;
      if (!proximity || !applied) throw new Error("Distance filter was not applied");
      return;
    }
    if (isPlayerSelectionAction(action)) {
      let expected: string[] | null | undefined;
      const saved = await options.updateSettings(settings => {
        // The shared save queue calls this again after a conflict or an earlier save.
        const current = state();
        if (!current.processingAvailable) throw new Error("Player processing is unavailable");
        if ("set_user_selected" in action && !playerUserId({ id: "", index: 0, user_id: action.set_user_selected.user_id })) {
          throw new Error("Player identity is unavailable");
        }
        const next = applyPlayerSelectionAction(settings, action, current.speakers, current.current);
        expected = next.audio.output.player_audio?.selected_tracks ?? null;
        return next;
      });
      const actual = saved.audio.output.player_audio?.selected_tracks ?? null;
      if (saved.audio.output.mode !== "vrchat_players" || expected === undefined
        || (expected === null ? actual !== null : actual === null
          || expected.length !== actual.length || expected.some(id => !actual.includes(id)))) {
        throw new Error("Player selection was not applied");
      }
    } else {
      options.updatePreferences(preferences => {
        const current = state();
        if ("set_track_hidden" in action && !current.speakers.some(speaker =>
          speaker.id === action.set_track_hidden.track_id && !speaker.id.startsWith("saved-user:"))) {
          throw new Error("Player track is unavailable");
        }
        if ("set_source_language_hidden" in action) {
          const language = sourceLanguageKey(action.set_source_language_hidden.language);
          if (!language || !current.languages.includes(language)) throw new Error("Source language is unavailable");
        }
        return applyPlayerVisibilityAction(preferences, action, current.defaultLanguages);
      });
    }
  };
}
