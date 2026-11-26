import { useCallback, useEffect, useRef, useState } from "react";

import type { AudioDevice } from "../../capture/types";
import { getCurrentHealth, subscribeHealth } from "../../core-client/health-store";
import type { Health } from "../../core-client/types";
import type { CoreHealthController } from "../../core-client/useCoreRuntime";
import type { ReportRuntimeError } from "../../core-client/useRuntimeErrors";
import { integrationsApi } from "../../integrations/api";
import { captureApi } from "../../capture/api";
import { settingsApi } from "../api";
import { createSettingsAutosave } from "../settings-autosave";
import type { Settings } from "../types";
import type { PlayerAudioStart } from "../../capture/usePlayerAudioStart";
import { withCurrentPlayerNames } from "../../capture/player-selection";
import { usesVrchatNetwork } from "../../capture/vrchat-network";
import { clearLegacyLearningBinding, learningPreferencesSnapshot, legacyLearningBinding } from "../../learning/preferences";

export function useSettingsRuntime({
  active,
  coreConfigured,
  audioSupported,
  refreshCapabilities,
  health,
  stopMicrophoneTest,
  setCaptureRunning,
  playerAudioStart,
  clearErrorFrom,
  reportError,
}: {
  active: boolean;
  coreConfigured: boolean;
  health: CoreHealthController;
  stopMicrophoneTest: () => Promise<void>;
  setCaptureRunning: (running: boolean) => Promise<boolean>;
  audioSupported: boolean | undefined;
  refreshCapabilities?: () => Promise<void>;
  playerAudioStart: PlayerAudioStart;
  clearErrorFrom: (source: string) => void;
  reportError: ReportRuntimeError;
}) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const persistedSettingsRef = useRef<Settings | null>(null);
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [devicesReady, setDevicesReady] = useState(false);
  const [devicesError, setDevicesError] = useState(false);

  const loadSettings = useCallback(async () => {
    if (!coreConfigured) return;
    const next = await settingsApi.settings();
    persistedSettingsRef.current = next;
    setSettings(next);
  }, [coreConfigured]);
  const loadSettingsRef = useRef(loadSettings);
  loadSettingsRef.current = loadSettings;

  const loadDevices = useCallback(async () => {
    if (!coreConfigured) return;
    if (audioSupported === undefined) return;
    if (!audioSupported) {
      setDevices([]);
      setDevicesReady(true);
      setDevicesError(false);
      return;
    }
    setDevicesError(false);
    try {
      setDevices(await captureApi.devices());
      setDevicesReady(true);
      setDevicesError(false);
    } catch {
      setDevicesReady(false);
      setDevicesError(true);
    }
  }, [audioSupported, clearErrorFrom, coreConfigured, reportError]);

  const refreshDevices = useCallback(async () => {
    if (!coreConfigured) return;
    await refreshCapabilities?.();
    await loadDevices();
  }, [coreConfigured, loadDevices, refreshCapabilities]);

  const refreshModels = loadSettings;

  useEffect(() => {
    if (!coreConfigured || settings !== null) return;
    let cancelled = false;
    let timer: number | null = null;
    const loadInitialResources = async () => {
      try {
        const nextSettings = await settingsApi.settings();
        if (cancelled) return;
        persistedSettingsRef.current = nextSettings;
        setSettings(nextSettings);
        clearErrorFrom("settings-bootstrap");
      } catch (reason) {
        if (cancelled) return;
        reportError(reason, "errors.core.connect", "settings-bootstrap");
        timer = window.setTimeout(() => void loadInitialResources(), 2500);
      }
    };
    void loadInitialResources();
    return () => {
      cancelled = true;
      if (timer !== null) window.clearTimeout(timer);
    };
  }, [clearErrorFrom, coreConfigured, reportError, settings]);

  useEffect(() => {
    if (active) void loadDevices();
  }, [active, loadDevices]);

  const persistSettings = useCallback(async (next: Settings): Promise<Settings> => {
    const previous = persistedSettingsRef.current;
    const networkActive = Boolean(health.getCurrent()?.network_capture_requested);
    const switchToPlayers = previous !== null && previous.audio.output.mode !== "vrchat_players"
      && next.audio.output.mode === "vrchat_players" && health.getCurrent()?.capture_requested && !networkActive;
    const enableNetwork = previous !== null && !usesVrchatNetwork(previous) && usesVrchatNetwork(next)
      && health.getCurrent()?.capture_requested && !networkActive;
    if (enableNetwork && !switchToPlayers) {
      // Core hot-enables Chatbox on save. Never submit it before preparation succeeds.
      if (!await playerAudioStart.confirm() || !await playerAudioStart.ensurePrivileges()) return previous;
    }
    // Stop before saving: Core normally restarts active inputs as part of saving settings.
    if (switchToPlayers) {
      if (!await playerAudioStart.confirm()) return previous;
      if (!await setCaptureRunning(false)) throw new Error("Could not pause capture before changing the audio source");
    }
    const microphoneRouteChanged = previous !== null && (
      previous.audio.microphone.mode !== next.audio.microphone.mode
      || previous.audio.microphone.device_id !== next.audio.microphone.device_id
      || previous.audio.microphone.device_key !== next.audio.microphone.device_key
    );
    if (health.getCurrent()?.microphone_test_running && microphoneRouteChanged) {
      try {
        await stopMicrophoneTest();
      } catch (reason) {
        clearErrorFrom("microphone-test");
        throw reason;
      }
    }
    let saved: Settings;
    try { saved = await settingsApi.saveSettings(next); }
    catch (error) {
      if (switchToPlayers) await setCaptureRunning(true).catch((reason) => reportError(reason, "errors.operation", "capture"));
      throw error;
    }
    persistedSettingsRef.current = saved;
    if (switchToPlayers) {
      let ready = false;
      try { ready = await playerAudioStart.ensurePrivileges(); }
      catch (reason) {
        // UAC cancellation leaves the original input and capture available.
        saved = await settingsApi.saveSettings({ ...saved, audio: { ...saved.audio, output: previous.audio.output } });
        persistedSettingsRef.current = saved;
        await setCaptureRunning(true).catch((error) => reportError(error, "errors.operation", "capture"));
        reportError(reason, "errors.operation", "capture");
      }
      if (ready) await setCaptureRunning(true).catch((reason) => reportError(reason, "errors.operation", "capture"));
    }
    void health.refreshQuietly();
    return saved;
  }, [health, stopMicrophoneTest, setCaptureRunning, playerAudioStart, reportError]);
  const persistSettingsRef = useRef(persistSettings);
  persistSettingsRef.current = persistSettings;

  const settingsAutosaveRef = useRef<ReturnType<typeof createSettingsAutosave<Settings>> | null>(null);
  if (settingsAutosaveRef.current === null) {
    settingsAutosaveRef.current = createSettingsAutosave<Settings>({
      persist: (next) => persistSettingsRef.current(next),
      getCurrent: () => persistedSettingsRef.current,
      reload: async () => {
        await loadSettingsRef.current();
        if (!persistedSettingsRef.current) throw new Error("Core is unavailable");
        return persistedSettingsRef.current;
      },
      onOptimistic: (next) => setSettings({
        ...next, features: persistedSettingsRef.current?.features ?? next.features,
      }),
      onCommit: (saved) => {
        setSettings(saved);
        clearErrorFrom("settings");
      },
      onError: (reason) => {
        if (persistedSettingsRef.current) setSettings(persistedSettingsRef.current);
        reportError(reason, "errors.settings.apply", "settings");
        void loadSettingsRef.current();
      },
    });
  }
  const settingsAutosave = settingsAutosaveRef.current;

  const migratingLearningBinding = useRef(false);
  useEffect(() => {
    if (!settings || migratingLearningBinding.current) return;
    const legacy = legacyLearningBinding(learningPreferencesSnapshot());
    if (!legacy) return;
    migratingLearningBinding.current = true;
    void settingsAutosaveRef.current!.update((latest) => {
      const binding = latest.learning_ai;
      if (binding && (!binding.use_default_model || binding.profile_id || binding.model)) return latest;
      return { ...latest, learning_ai: legacy };
    }).then(clearLegacyLearningBinding)
      .catch(() => { /* Keep the browser binding until it is stored in Core. */ })
      .finally(() => { migratingLearningBinding.current = false; });
  }, [settings]);

  const refreshingPlayerNames = useRef(false);
  const syncObservedPlayers = useCallback((players: NonNullable<Health["player_audio"]>["players"] | undefined) => {
    // This runtime stays mounted on every page. Use the shared autosave queue so
    // a name refresh cannot overwrite concurrent settings or restore a deselection.
    if (!players?.length || refreshingPlayerNames.current) return;
    const current = persistedSettingsRef.current;
    const speakers = players.map((player) => player.speaker);
    if (!current || withCurrentPlayerNames(current, speakers) === current) return;
    refreshingPlayerNames.current = true;
    void settingsAutosaveRef.current!.update((latest) => withCurrentPlayerNames(latest, speakers))
      .catch(() => { /* Shared settings errors report failures; the next health update retries. */ })
      .finally(() => { refreshingPlayerNames.current = false; });
  }, []);
  const observedPlayersFrom = (current: Health | null) => current?.network_players ?? current?.network_capture?.players ?? current?.player_audio?.players;
  const observedPlayers = observedPlayersFrom(health.value);
  useEffect(() => {
    syncObservedPlayers(observedPlayers);
  }, [observedPlayers, syncObservedPlayers]);
  useEffect(() => {
    if (settings) syncObservedPlayers(observedPlayersFrom(getCurrentHealth()));
  }, [settings, syncObservedPlayers]);
  useEffect(() => subscribeHealth(() => {
    syncObservedPlayers(observedPlayersFrom(getCurrentHealth()));
  }), [syncObservedPlayers]);

  const testOsc = useCallback(async () => {
    await integrationsApi.testOsc();
    clearErrorFrom("osc");
    window.setTimeout(() => void health.refreshQuietly(), 200);
  }, [clearErrorFrom, health]);

  return {
    value: settings,
    getCurrent: () => persistedSettingsRef.current,
    getOptimistic: () => settingsAutosave.getDesired(),
    devices: { items: devices, ready: devicesReady, error: devicesError, refresh: refreshDevices },
    asr: { refresh: refreshModels },
    refresh: loadSettings,
    reload: async () => { await loadSettings(); return persistedSettingsRef.current ?? undefined; },
    save: settingsAutosave,
    update: settingsAutosave.update,
    testOsc,
  };
}
