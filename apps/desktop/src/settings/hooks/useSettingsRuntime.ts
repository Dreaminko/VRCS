import { useCallback, useEffect, useRef, useState } from "react";

import type { AudioDevice } from "../../capture/types";
import type { CoreHealthController } from "../../core-client/useCoreRuntime";
import type { ReportRuntimeError } from "../../core-client/useRuntimeErrors";
import { integrationsApi } from "../../integrations/api";
import { providersApi } from "../../providers/api";
import type { AsrCapabilities } from "../../providers/types";
import { captureApi } from "../../capture/api";
import { settingsApi } from "../api";
import { createSettingsAutosave } from "../settings-autosave";
import type { Settings } from "../types";

export function useSettingsRuntime({
  active,
  coreConfigured,
  health,
  stopMicrophoneTest,
  clearErrorFrom,
  reportError,
}: {
  active: boolean;
  coreConfigured: boolean;
  health: CoreHealthController;
  stopMicrophoneTest: () => Promise<void>;
  clearErrorFrom: (source: string) => void;
  reportError: ReportRuntimeError;
}) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const persistedSettingsRef = useRef<Settings | null>(null);
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [devicesReady, setDevicesReady] = useState(false);
  const [asrCapabilities, setAsrCapabilities] = useState<AsrCapabilities | null>(null);

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
    try {
      setDevices(await captureApi.devices());
      setDevicesReady(true);
      clearErrorFrom("devices");
    } catch (reason) {
      setDevicesReady(false);
      reportError(reason, "errors.audio.devices", "devices");
    }
  }, [clearErrorFrom, coreConfigured, reportError]);

  const loadAsrCapabilities = useCallback(async () => {
    if (!coreConfigured) return;
    try {
      setAsrCapabilities(await providersApi.asrCapabilities());
      clearErrorFrom("asr");
    } catch (reason) {
      reportError(reason, "errors.asr.capabilities", "asr");
    }
  }, [clearErrorFrom, coreConfigured, reportError]);
  const loadAsrCapabilitiesRef = useRef(loadAsrCapabilities);
  loadAsrCapabilitiesRef.current = loadAsrCapabilities;

  useEffect(() => {
    if (!coreConfigured || settings !== null) return;
    let cancelled = false;
    let timer: number | null = null;
    const loadInitialResources = async () => {
      try {
        const [nextSettings, nextCapabilities] = await Promise.all([
          settingsApi.settings(),
          providersApi.asrCapabilities(),
        ]);
        if (cancelled) return;
        persistedSettingsRef.current = nextSettings;
        setSettings(nextSettings);
        setAsrCapabilities(nextCapabilities);
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
    if (active) void Promise.all([loadDevices(), loadAsrCapabilities()]);
  }, [active, loadAsrCapabilities, loadDevices]);

  const persistSettings = useCallback(async (next: Settings): Promise<Settings> => {
    const previous = persistedSettingsRef.current;
    const microphoneRouteChanged = previous !== null && (
      previous.audio.microphone.mode !== next.audio.microphone.mode
      || previous.audio.microphone.device_id !== next.audio.microphone.device_id
    );
    if (health.getCurrent()?.microphone_test_running && microphoneRouteChanged) {
      await stopMicrophoneTest();
    }
    const saved = await settingsApi.saveSettings(next);
    persistedSettingsRef.current = saved;
    void health.refreshQuietly();
    return saved;
  }, [health, stopMicrophoneTest]);
  const persistSettingsRef = useRef(persistSettings);
  persistSettingsRef.current = persistSettings;

  const settingsAutosaveRef = useRef<ReturnType<typeof createSettingsAutosave<Settings>> | null>(null);
  if (settingsAutosaveRef.current === null) {
    settingsAutosaveRef.current = createSettingsAutosave<Settings>({
      persist: (next) => persistSettingsRef.current(next),
      onOptimistic: setSettings,
      onCommit: () => {
        clearErrorFrom("settings");
        void loadAsrCapabilitiesRef.current();
      },
      onError: (reason) => {
        if (persistedSettingsRef.current) setSettings(persistedSettingsRef.current);
        reportError(reason, "errors.settings.apply", "settings");
        void loadSettingsRef.current();
      },
    });
  }

  const testOsc = useCallback(async () => {
    await integrationsApi.testOsc();
    clearErrorFrom("osc");
    window.setTimeout(() => void health.refreshQuietly(), 200);
  }, [clearErrorFrom, health]);

  return {
    value: settings,
    devices: { items: devices, ready: devicesReady, refresh: loadDevices },
    asr: { capabilities: asrCapabilities, refresh: loadAsrCapabilities },
    refresh: loadSettings,
    save: settingsAutosaveRef.current,
    testOsc,
  };
}
