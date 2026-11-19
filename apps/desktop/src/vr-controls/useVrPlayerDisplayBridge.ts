import { useEffect, useMemo, useRef } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import type { AppWorkspace } from "../app/useAppWorkspace";
import type { PlayerSourceOverride } from "../capture/types";
import { playerUserId } from "../capture/player-selection";
import type { PlayerAudioStatus } from "../core-client/types";
import { collectPlayerSpeakers, playerLanguageOptions, sourceMode } from "../subtitles/player-controls";
import { defaultHiddenSourceLanguages, isAudioTrack } from "../subtitles/player-history";
import { useSpeakerIdentities } from "../subtitles/speaker-state";
import type { SpeakerIdentity, Subtitle } from "../subtitles/types";
import { localizedLanguageName } from "../translation/translation-languages";
import { createPlayerDisplayExecutor, createPlayerDisplayPublisher, createPlayerDisplayReceiver, type PlayerDisplayRequest, type PlayerDisplayView } from "./player-display-protocol";

const EMPTY_NAMES: Record<string, string> = {};
const EMPTY_OVERRIDES: Record<string, PlayerSourceOverride> = {};
const EMPTY_ROSTER: NonNullable<PlayerAudioStatus["players"]> = [];
const EMPTY_SPEAKERS: SpeakerIdentity[] = [];
const EMPTY_LANGUAGES: string[] = [];

interface CachedPlayerLists {
  subtitles: Subtitle[];
  roster: NonNullable<PlayerAudioStatus["players"]>;
  overrides: Record<string, PlayerSourceOverride>;
  names: Record<string, string>;
  defaultLanguages: string[];
  hiddenSourceLanguages: string[] | null;
  observedSpeakers: SpeakerIdentity[];
  languageOptions: string[];
}

function playerDisplayState(workspace: AppWorkspace, optimistic = false, cached?: CachedPlayerLists) {
  const confirmed = workspace.settings.getCurrent();
  const settings = optimistic
    ? workspace.settings.getOptimistic() ?? workspace.settings.value ?? confirmed
    : confirmed;
  const playerMode = settings?.audio.output.mode === "vrchat_players";
  const names = settings?.audio.output.player_audio?.selected_user_names ?? EMPTY_NAMES;
  const overrides = settings?.audio.output.player_audio?.source_overrides ?? EMPTY_OVERRIDES;
  const unmatchedBlocked = settings?.audio.output.player_audio?.source_mode !== "everyone";
  const proximity = settings?.audio.output.player_audio?.proximity;
  const viewPriority = settings?.audio.output.player_audio?.view_priority;
  const roster = workspace.runtime.health?.network_players
    ?? (playerMode ? workspace.runtime.health?.player_audio?.players : undefined)
    ?? EMPTY_ROSTER;
  const current = roster.map(player => player.speaker);
  const distances = new Map(roster.map(player => [player.speaker.id, player.distance_m]));
  const savedIds = Object.keys(overrides);
  const preferences = workspace.playerDisplay.getCurrent();
  const defaultLanguages = defaultHiddenSourceLanguages(settings?.translation, workspace.runtime.health?.language_session);
  const available = workspace.onboarding.status === "complete";
  const speakers = cached
    && cached.subtitles === workspace.subtitles.items
    && cached.roster === roster
    && cached.overrides === overrides
    && cached.names === names
    ? cached.observedSpeakers
    : collectPlayerSpeakers(workspace.subtitles.items, current, savedIds, names);
  const languages = cached
    && cached.subtitles === workspace.subtitles.items
    && cached.defaultLanguages === defaultLanguages
    && cached.hiddenSourceLanguages === preferences.hiddenSourceLanguages
    ? cached.languageOptions
    : playerLanguageOptions(workspace.subtitles.items, defaultLanguages, preferences.hiddenSourceLanguages);
  return {
    available, processingAvailable: available && workspace.runtime.ready && playerMode,
    playerMode, sourceMode: sourceMode(settings?.audio.output.player_audio), names, overrides, proximity, viewPriority, current, distances, preferences, defaultLanguages, unmatchedBlocked,
    hiddenTracks: preferences.hiddenTracks,
    speakers,
    languages,
  };
}

export function useVrPlayerDisplayBridge(epoch: number, workspace: AppWorkspace) {
  const { t, i18n } = useTranslation();
  const active = Boolean(epoch && isTauri());
  const locale = i18n.resolvedLanguage ?? "en-US";
  const confirmedSettings = workspace.settings.getCurrent();
  const activeSettings = workspace.settings.getOptimistic() ?? workspace.settings.value ?? confirmedSettings;
  const playerMode = activeSettings?.audio.output.mode === "vrchat_players";
  const names = activeSettings?.audio.output.player_audio?.selected_user_names ?? EMPTY_NAMES;
  const overrides = activeSettings?.audio.output.player_audio?.source_overrides ?? EMPTY_OVERRIDES;
  const roster = workspace.runtime.health?.network_players
    ?? (playerMode ? workspace.runtime.health?.player_audio?.players : undefined)
    ?? EMPTY_ROSTER;
  const viewHealth = workspace.runtime.health?.player_audio?.proximity;
  const languageSession = workspace.runtime.health?.language_session;
  const subtitles = workspace.subtitles.items;
  const defaultLanguages = workspace.playerDisplay.defaultLanguages;
  const hiddenSourceLanguages = workspace.playerDisplay.preferences.hiddenSourceLanguages;

  const currentSpeakers = useMemo(() => (active ? roster.map((player) => player.speaker) : EMPTY_SPEAKERS), [active, roster]);
  const savedIds = useMemo(() => (active ? Object.keys(overrides) : EMPTY_LANGUAGES), [active, overrides]);
  const observedSpeakers = useMemo(
    () => (active ? collectPlayerSpeakers(subtitles, currentSpeakers, savedIds, names) : EMPTY_SPEAKERS),
    [active, subtitles, currentSpeakers, savedIds, names],
  );
  const languageOptions = useMemo(
    () => (active ? playerLanguageOptions(subtitles, defaultLanguages, hiddenSourceLanguages) : EMPTY_LANGUAGES),
    [active, subtitles, defaultLanguages, hiddenSourceLanguages],
  );
  const identities = useSpeakerIdentities(observedSpeakers);
  const cachedRef = useRef<CachedPlayerLists | undefined>(undefined);
  cachedRef.current = active
    ? {
        subtitles,
        roster,
        overrides,
        names,
        defaultLanguages,
        hiddenSourceLanguages,
        observedSpeakers,
        languageOptions,
      }
    : undefined;
  const current = useRef({ workspace, t, locale, identities });
  current.current = { workspace, t, locale, identities };
  const publisher = useRef<(() => Promise<void>) | null>(null);
  const order = useRef(new Map<string, number>());
  const nextOrder = useRef(0);

  const readState = (optimistic = false) => {
    const latest = current.current;
    const state = playerDisplayState(latest.workspace, optimistic, cachedRef.current);
    const resolved = new Map(latest.identities.map(identity => [identity.id, identity]));
    const speakers = state.speakers.map(speaker => {
      if (speaker.id.startsWith("saved-user:")) return speaker;
      const identity = resolved.get(speaker.id);
      return identity ? { ...speaker, display_name: identity.display_name ?? speaker.display_name, user_id: identity.user_id ?? speaker.user_id } : speaker;
    });
    const actual = new Map(speakers.map(speaker => [speaker.id, speaker]));
    return { ...state, speakers, current: state.current.map(speaker => actual.get(speaker.id) ?? speaker) };
  };

  const buildView = (): PlayerDisplayView => {
    const state = readState(true);
    const { t: translate, locale } = current.current;
    const currentIds = new Set(state.current.map(speaker => speaker.id));
    const known = new Set(state.speakers.map(speaker => speaker.id));
    for (const id of order.current.keys()) if (!known.has(id)) order.current.delete(id);
    for (const id of known) if (!order.current.has(id)) order.current.set(id, nextOrder.current++);
    const labels: Record<string, string> = {
      ...(translate("live.players", { returnObjects: true }) as Record<string, string>),
      close: translate("common.close"), previous: translate("wristReader.previousPage"),
      next: translate("wristReader.nextPage"), busy: translate("settings.vrOverlay.controlBusy"),
    };
    const hiddenLanguages = state.preferences.hiddenSourceLanguages ?? [];
    const hiddenUserIds = new Set(state.speakers
      .filter(speaker => state.hiddenTracks.includes(speaker.id))
      .map(playerUserId)
      .filter((id): id is string => id !== null));
    const languageName = (language: string) => localizedLanguageName(language, locale);
    const viewHealth = current.current.workspace.runtime.health?.player_audio?.proximity;
    return {
      available: state.available,
      processing_available: state.processingAvailable,
      player_mode: state.playerMode,
      source_mode: state.sourceMode,
      ...(state.proximity ? { proximity: { enabled: state.proximity.enabled, radius_m: state.proximity.radius_m } } : {}),
      ...(state.viewPriority ? { view_priority: {
        enabled: state.viewPriority.enabled, horizontal_fov_deg: state.viewPriority.horizontal_fov_deg,
        local_view: state.processingAvailable && viewHealth?.view_enabled === true && viewHealth.local_view,
      } } : {}),
      players: [...state.speakers].sort((a, b) => order.current.get(a.id)! - order.current.get(b.id)!).map(speaker => {
        const userId = playerUserId(speaker);
        const saved = speaker.id.startsWith("saved-user:");
        const isCurrent = currentIds.has(speaker.id);
        const number = translate(isAudioTrack(speaker) || isCurrent ? "live.trackNumber" : "live.speakerNumber", { number: speaker.index + 1 });
        const name = speaker.id === "other:unidentified" ? labels.unidentified
          : speaker.display_name || (userId && state.names[userId]) || userId || number;
        const category = isCurrent ? labels.current : saved ? labels.savedSelection : labels.history;
        const distance = state.distances.get(speaker.id);
        let detail: string;
        if (isCurrent) {
          if (!userId) {
            detail = `${category} · ${number} · ${labels.userIdPending}`;
          } else if (distance !== undefined && Number.isFinite(distance)) {
            const dist = translate("live.players.distance", { distance: distance.toFixed(1) });
            const far = state.proximity?.enabled && distance > state.proximity.radius_m;
            detail = `${category} · ${dist}${far ? ` · ${labels.outOfRange}` : ""}`;
          } else if (state.playerMode) {
            detail = `${category} · ${labels.distanceUnknown}`;
          } else {
            detail = `${category} · ${number}`;
          }
        } else {
          detail = saved ? `${category} · ${userId?.slice(-8) ?? ""}` : `${category} · ${number}`;
        }
        const rule = userId ? state.overrides[userId] : undefined;
        return {
          track_id: saved ? null : speaker.id, user_id: userId, name, detail,
          current: isCurrent,
          hidden: !saved && state.preferences.hiddenTracks.includes(speaker.id),
          chatbox_disabled: rule ? rule.chatbox_disabled : state.unmatchedBlocked, voice_disabled: rule ? rule.voice_disabled : state.unmatchedBlocked,
          voice_locked: Boolean(userId && hiddenUserIds.has(userId)),
        };
      }),
      languages: state.languages.map(code => ({ code, label: languageName(code), hidden: hiddenLanguages.includes(code) })),
      follow_default: false,
      default_languages: state.defaultLanguages.map(languageName),
      labels, busy: false, error: null,
    };
  };
  const latest = useRef({ readState, buildView });
  latest.current = { readState, buildView };

  useEffect(() => {
    if (!epoch || !isTauri()) return;
    let disposed = false;
    let stop: (() => void) | undefined;
    const statePublisher = createPlayerDisplayPublisher({
      // Saving can finish before React renders: always read committed settings here.
      getView: () => latest.current.buildView(),
      update: (revision, view) => invoke("vr_overlay_player_display_update", { epoch, revision, view }),
    });
    const publish = async () => {
      if (disposed) return;
      await statePublisher.publish();
    };
    const execute = createPlayerDisplayExecutor({
      getState: () => {
        const state = latest.current.readState();
        return disposed ? { ...state, available: false } : state;
      },
      updateSettings: apply => current.current.workspace.settings.update(apply),
      updatePreferences: apply => current.current.workspace.playerDisplay.update(apply),
    });
    const receive = createPlayerDisplayReceiver({
      epoch, active: () => !disposed,
      execute, publish,
      finish: (request, error) => invoke("vr_overlay_player_display_result", { request, error }),
    });
    void (async () => {
      stop = await listen<PlayerDisplayRequest>("vr-overlay-player-display-request", ({ payload }) => {
        void receive(payload).catch(error => console.warn("VR player display result failed", error));
      });
      if (disposed) { stop(); return; }
      // Native must not expose editable state until its request listener is ready.
      publisher.current = publish;
      await publish();
    })().catch(error => console.warn("VR player display registration failed", error));
    return () => {
      disposed = true;
      if (publisher.current === publish) publisher.current = null;
      stop?.();
    };
  }, [epoch]);

  useEffect(() => {
    if (!epoch) return;
    void publisher.current?.().catch(error => console.warn("VR player display state update failed", error));
  }, [
    epoch,
    activeSettings,
    roster,
    viewHealth,
    languageSession,
    observedSpeakers,
    languageOptions,
    workspace.playerDisplay.preferences,
    defaultLanguages,
    identities,
    workspace.onboarding.status,
    workspace.runtime.ready,
    locale,
    t,
  ]);
}
