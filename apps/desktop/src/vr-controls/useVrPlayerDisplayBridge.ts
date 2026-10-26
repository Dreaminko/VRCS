import { useEffect, useRef } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import type { AppWorkspace } from "../app/useAppWorkspace";
import { playerUserId } from "../capture/player-selection";
import { collectPlayerSpeakers, playerLanguageOptions } from "../subtitles/player-controls";
import { defaultHiddenSourceLanguages, isAudioTrack } from "../subtitles/player-history";
import { useSpeakerIdentities } from "../subtitles/speaker-state";
import { localizedLanguageName } from "../translation/translation-languages";
import { createPlayerDisplayExecutor, createPlayerDisplayPublisher, createPlayerDisplayReceiver, type PlayerDisplayRequest, type PlayerDisplayView } from "./player-display-protocol";

function playerDisplayState(workspace: AppWorkspace) {
  const settings = workspace.settings.getCurrent();
  const playerMode = settings?.audio.output.mode === "vrchat_players";
  const selected = settings?.audio.output.player_audio?.selected_tracks ?? null;
  const names = settings?.audio.output.player_audio?.selected_user_names ?? {};
  const current = playerMode ? (workspace.runtime.health?.player_audio?.players ?? []).map(player => player.speaker) : [];
  const preferences = workspace.playerDisplay.getCurrent();
  const defaultLanguages = defaultHiddenSourceLanguages(settings?.translation, workspace.runtime.health?.language_session);
  const available = workspace.onboarding.status === "complete";
  return {
    available, processingAvailable: available && workspace.runtime.ready && playerMode,
    playerMode, selected, names, current, preferences, defaultLanguages,
    speakers: collectPlayerSpeakers(workspace.subtitles.items, current, selected, names),
    languages: playerLanguageOptions(workspace.subtitles.items, defaultLanguages, preferences.hiddenSourceLanguages),
  };
}

export function useVrPlayerDisplayBridge(epoch: number, workspace: AppWorkspace) {
  const { t, i18n } = useTranslation();
  const observed = playerDisplayState(workspace);
  const identities = useSpeakerIdentities(observed.speakers);
  const current = useRef({ workspace, t, locale: i18n.resolvedLanguage ?? "en-US", identities });
  current.current = { workspace, t, locale: i18n.resolvedLanguage ?? "en-US", identities };
  const publisher = useRef<(() => Promise<void>) | null>(null);
  const order = useRef(new Map<string, number>());
  const nextOrder = useRef(0);

  const readState = () => {
    const latest = current.current;
    const state = playerDisplayState(latest.workspace);
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
    const state = readState();
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
    const hiddenLanguages = state.preferences.hiddenSourceLanguages ?? state.defaultLanguages;
    const languageName = (language: string) => localizedLanguageName(language, locale);
    return {
      available: state.available,
      processing_available: state.processingAvailable,
      player_mode: state.playerMode,
      selection_mode: state.selected === null ? "all" : "selected",
      selected_count: state.selected?.length ?? 0,
      players: [...state.speakers].sort((a, b) => order.current.get(a.id)! - order.current.get(b.id)!).map(speaker => {
        const userId = playerUserId(speaker);
        const saved = speaker.id.startsWith("saved-user:");
        const isCurrent = currentIds.has(speaker.id);
        const number = translate(isAudioTrack(speaker) || isCurrent ? "live.trackNumber" : "live.speakerNumber", { number: speaker.index + 1 });
        const name = speaker.id === "other:unidentified" ? labels.unidentified
          : speaker.display_name || (userId && state.names[userId]) || userId || number;
        const category = isCurrent ? labels.current : saved ? labels.savedSelection : labels.history;
        const detail = saved ? `${category} - ${userId?.slice(-8) ?? ""}`
          : `${category} - ${number}${isCurrent && !userId ? ` - ${labels.userIdPending}` : ""}`;
        return {
          track_id: saved ? null : speaker.id, user_id: userId, name, detail,
          current: isCurrent, selected: state.selected === null || Boolean(userId && state.selected.includes(userId)),
          hidden: !saved && state.preferences.hiddenTracks.includes(speaker.id),
        };
      }),
      languages: state.languages.map(code => ({ code, label: languageName(code), hidden: hiddenLanguages.includes(code) })),
      follow_default: state.preferences.hiddenSourceLanguages === null,
      default_languages: state.defaultLanguages.map(languageName),
      labels, busy: false, error: null,
    };
  };
  const latest = useRef({ readState, buildView });
  latest.current = { readState, buildView };
  const snapshot = JSON.stringify(buildView());

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
      epoch, revision: statePublisher.revision, active: () => !disposed,
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
    void publisher.current?.().catch(error => console.warn("VR player display state update failed", error));
  }, [epoch, snapshot]);
}
