import type { Settings } from "../settings/types";
import type { SpeakerIdentity } from "../subtitles/types";
import type { PlayerSourceMode } from "../capture/types";
import { playerUserId } from "../capture/player-selection.ts";
import { sourceLanguageKey, type PlayerHistoryPreferences } from "../subtitles/player-history.ts";
import { applyPlayerProximity, applyPlayerSourceAction, applyPlayerSourceMode, applyPlayerViewPriority, applyPlayerVisibilityAction, isPlayerProximityAction, isPlayerSourceAction, isPlayerSourceModeAction, isPlayerViewPriorityAction, sourceMode, type PlayerDisplayAction } from "../subtitles/player-controls.ts";
import type { ControlAction, ControlSource } from "./protocol";

export interface PlayerDisplayRow {
  track_id: string | null;
  user_id: string | null;
  name: string;
  detail: string;
  current: boolean;
  hidden: boolean;
  chatbox_disabled: boolean;
  voice_disabled: boolean;
  voice_locked: boolean;
}

export interface PlayerDisplayView {
  available: boolean;
  processing_available: boolean;
  player_mode: boolean;
  source_mode: PlayerSourceMode;
  players: PlayerDisplayRow[];
  proximity?: {
    enabled: boolean;
    radius_m: number;
  };
  view_priority?: {
    enabled: boolean;
    horizontal_fov_deg: number;
    local_view: boolean;
  };
  languages: Array<{ code: string; label: string; hidden: boolean }>;
  follow_default: boolean;
  default_languages: string[];
  labels: Record<string, string>;
  busy: boolean;
  error: string | null;
}

export interface WristMenuUi {
  page: "home" | "players" | "rules" | "languages";
  group: "current" | "history";
  player_page: number;
  language_page: number;
}

export interface SliderCommand {
  action: "proximity_radius" | "view_horizontal_fov";
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
  | { slider: SliderCommand }
  | { display: PlayerDisplayAction }
  | "dashboard";

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
  hiddenTracks: readonly string[];
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
    if (isPlayerProximityAction(action)) {
      if (!Number.isFinite(action.set_proximity_radius)) throw new Error("Distance limit is invalid");
      const radius_m = Math.round(action.set_proximity_radius * 10) / 10;
      if (radius_m < 0 || radius_m > 10) throw new Error("Distance limit is invalid");
      const saved = await options.updateSettings(settings => {
        if (settings.audio.output.mode !== "vrchat_players") throw new Error("Player processing is unavailable");
        const proximity = settings.audio.output.player_audio?.proximity;
        if (!proximity) throw new Error("Distance filtering is unavailable");
        return proximity.radius_m === radius_m && proximity.enabled === (radius_m > 0)
          ? settings : applyPlayerProximity(settings, radius_m);
      });
      const proximity = saved.audio.output.player_audio?.proximity;
      if (!proximity || proximity.enabled !== (radius_m > 0) || Math.abs(proximity.radius_m - radius_m) >= 0.001) {
        throw new Error("Distance filter was not applied");
      }
      return;
    }
    if (isPlayerViewPriorityAction(action)) {
      const { enabled, horizontal_fov_deg } = action.set_view_priority;
      if (!Number.isFinite(horizontal_fov_deg) || horizontal_fov_deg < 30 || horizontal_fov_deg > 180) {
        throw new Error("Horizontal view is invalid");
      }
      const saved = await options.updateSettings(settings => {
        if (settings.audio.output.mode !== "vrchat_players") throw new Error("Player processing is unavailable");
        if (!settings.audio.output.player_audio?.view_priority) throw new Error("View priority is unavailable");
        return applyPlayerViewPriority(settings, enabled, horizontal_fov_deg);
      });
      const view = saved.audio.output.player_audio?.view_priority;
      if (!view || view.enabled !== enabled || Math.abs(view.horizontal_fov_deg - horizontal_fov_deg) >= 0.001) {
        throw new Error("View priority was not applied");
      }
      return;
    }
    if (isPlayerSourceModeAction(action)) {
      const mode = action.set_source_mode;
      if (mode !== "everyone" && mode !== "allowlist") throw new Error("Player source mode is invalid");
      const saved = await options.updateSettings(settings => applyPlayerSourceMode(settings, mode));
      if (sourceMode(saved.audio.output.player_audio) !== mode) throw new Error("Player source mode was not applied");
      return;
    }
    if (isPlayerSourceAction(action)) {
      const { user_id, source, disabled } = action.set_user_source_disabled;
      if (source === "speaker" && !disabled && state().speakers.some(speaker =>
        playerUserId(speaker) === user_id && state().hiddenTracks.includes(speaker.id))) {
        throw new Error("Unhide the player before enabling voice");
      }
      const saved = await options.updateSettings(settings => applyPlayerSourceAction(settings, action));
      const unmatched = sourceMode(saved.audio.output.player_audio) === "allowlist";
      const rule = saved.audio.output.player_audio?.source_overrides?.[user_id];
      const effective = rule ? (source === "vrchat_chatbox" ? rule.chatbox_disabled : rule.voice_disabled) : unmatched;
      if (effective !== disabled) throw new Error("Player source setting was not applied");
    } else if ("set_track_hidden" in action) {
      const current = state();
      const speaker = current.speakers.find(candidate =>
        candidate.id === action.set_track_hidden.track_id && !candidate.id.startsWith("saved-user:"));
      if (!speaker) throw new Error("Player track is unavailable");
      const userId = playerUserId(speaker);
      // Hiding also stops voice capture; unhiding never enables a muted player.
      if (userId && action.set_track_hidden.hidden) {
        const saved = await options.updateSettings(settings => applyPlayerSourceAction(settings,
          { set_user_source_disabled: { user_id: userId, source: "speaker", disabled: true } }, speaker));
        const rule = saved.audio.output.player_audio?.source_overrides?.[userId];
        const unmatched = sourceMode(saved.audio.output.player_audio) === "allowlist";
        if (!(rule ? rule.voice_disabled : unmatched)) throw new Error("Player source setting was not applied");
      }
      options.updatePreferences(preferences => applyPlayerVisibilityAction(preferences, action, current.defaultLanguages));
    } else {
      options.updatePreferences(preferences => {
        const current = state();
        if ("set_source_language_hidden" in action) {
          const language = sourceLanguageKey(action.set_source_language_hidden.language);
          if (!language || !current.languages.includes(language)) throw new Error("Source language is unavailable");
        }
        return applyPlayerVisibilityAction(preferences, action, current.defaultLanguages);
      });
    }
  };
}
