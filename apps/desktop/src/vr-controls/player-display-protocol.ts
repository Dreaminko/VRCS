import type { Settings } from "../settings/types";
import type { SpeakerIdentity } from "../subtitles/types";
import { playerUserId } from "../capture/player-selection.ts";
import { sourceLanguageKey, type PlayerHistoryPreferences } from "../subtitles/player-history.ts";
import { applyPlayerSelectionAction, applyPlayerVisibilityAction, isPlayerSelectionAction, type PlayerDisplayAction } from "../subtitles/player-controls.ts";
import type { ControlAction, ControlSource } from "./protocol";

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
  languages: Array<{ code: string; label: string; hidden: boolean }>;
  follow_default: boolean;
  default_languages: string[];
  labels: Record<string, string>;
  busy: boolean;
  error: string | null;
}

export interface WristMenuUi {
  page: "home" | "players" | "languages";
  group: "current" | "history";
  player_page: number;
  language_page: number;
}

export type WristMenuCommand =
  | { control: ControlAction }
  | { page: WristMenuUi["page"] }
  | { group: WristMenuUi["group"] }
  | { player_page: number }
  | { language_page: number }
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
  revision: () => number;
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
      if (request.base_revision !== options.revision()) throw new Error("State changed; try again");
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
}) {
  const state = () => {
    const current = options.getState();
    if (!current.available) throw new Error("Player display is unavailable");
    return current;
  };
  return async (action: PlayerDisplayAction): Promise<void> => {
    state();
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
