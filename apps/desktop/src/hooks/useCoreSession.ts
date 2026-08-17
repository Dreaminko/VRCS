import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  coreApi,
  coreStartup,
  initializeCoreApi,
  retryCore as retryCoreStartup,
} from "../api";
import { localizedError } from "../app-utils";
import { createSettingsAutosave } from "../settings-autosave";
import type {
  AsrCapabilities,
  AudioDevice,
  DictionarySource,
  Health,
  Settings,
} from "../types";
import { useSubtitleStream } from "./useSubtitleStream";

export function useCoreSession(settingsPageActive: boolean) {
  const { t } = useTranslation();
  const tRef = useRef(t);
  tRef.current = t;

  const [coreConfigured, setCoreConfigured] = useState(false);
  const [startupState, setStartupState] = useState<"starting" | "ready" | "failed">("starting");
  const [startupAttempt, setStartupAttempt] = useState(0);
  const [health, setHealth] = useState<Health | null>(null);
  const [capturePending, setCapturePending] = useState(false);
  const capturePendingRef = useRef(false);
  const healthRef = useRef<Health | null>(null);
  healthRef.current = health;
  const [settings, setSettings] = useState<Settings | null>(null);
  const persistedSettingsRef = useRef<Settings | null>(null);
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [devicesReady, setDevicesReady] = useState(false);
  const [asrCapabilities, setAsrCapabilities] = useState<AsrCapabilities | null>(null);
  const [dictionarySources, setDictionarySources] = useState<DictionarySource[]>([]);
  const [errors, setErrors] = useState<Map<string, string>>(() => new Map());
  const error = [...errors.values()].at(-1) ?? null;

  const reportError = useCallback((
    reason: unknown,
    fallbackKey: string,
    source = "general",
  ) => {
    const message = localizedError(reason, tRef.current, fallbackKey);
    setErrors((current) => {
      const next = new Map(current);
      next.delete(source);
      next.set(source, message);
      return next;
    });
  }, []);

  const clearError = useCallback(() => {
    setErrors(new Map());
  }, []);

  const clearErrorFrom = useCallback((source: string) => {
    setErrors((current) => {
      if (!current.has(source)) return current;
      const next = new Map(current);
      next.delete(source);
      return next;
    });
  }, []);

  const {
    connection: streamConnection,
    openedConversationId,
    subtitles,
    hasOlderSubtitles,
    loadingConversationSubtitles,
    loadingOlderSubtitles,
    conversationCatalogEvent,
    openConversation,
    loadOlderSubtitles,
    vrchatMuteStatus,
    translatingSubtitleIds,
    clearPartials,
    translateSubtitle,
  } = useSubtitleStream({
    coreConfigured,
    reportError,
    clearErrorFrom,
  });
  const connection = startupState === "failed" ? "disconnected" : streamConnection;

  useEffect(() => {
    let cancelled = false;
    let timer: number | null = null;
    const pollStartup = async () => {
      try {
        const startup = await coreStartup();
        if (cancelled) return;
        setStartupState(startup.state);
        if (startup.state === "ready") {
          setCoreConfigured(true);
          return;
        }
        if (startup.state === "failed") {
          setErrors((current) => {
            const next = new Map(current);
            next.set("core", tRef.current("errors.core.initialize"));
            return next;
          });
          return;
        }
        timer = window.setTimeout(() => void pollStartup(), 150);
      } catch (reason) {
        if (!cancelled) {
          setStartupState("failed");
          reportError(reason, "errors.core.initialize", "core");
        }
      }
    };
    void initializeCoreApi().then(pollStartup).catch((reason) => {
      if (!cancelled) {
        setStartupState("failed");
        reportError(reason, "errors.core.initialize", "core");
      }
    });
    return () => {
      cancelled = true;
      if (timer !== null) window.clearTimeout(timer);
    };
  }, [reportError, startupAttempt]);

  const retryCore = useCallback(async () => {
    clearError();
    setCoreConfigured(false);
    setStartupState("starting");
    try {
      await retryCoreStartup();
      setStartupAttempt((attempt) => attempt + 1);
    } catch (reason) {
      setStartupState("failed");
      reportError(reason, "errors.core.initialize", "core");
    }
  }, [clearError, reportError]);

  const loadSettings = useCallback(async () => {
    if (!coreConfigured) return;
    const nextSettings = await coreApi.settings();
    persistedSettingsRef.current = nextSettings;
    setSettings(nextSettings);
  }, [coreConfigured]);
  const loadSettingsRef = useRef(loadSettings);
  loadSettingsRef.current = loadSettings;

  useEffect(() => {
    if (!coreConfigured) return;
    let cancelled = false;
    let timer: number | null = null;
    const poll = async () => {
      if (settings === null) {
        try {
          const [nextHealth, nextSettings, nextAsrCapabilities] = await Promise.all([
            coreApi.health(),
            coreApi.settings(),
            coreApi.asrCapabilities(),
          ]);
          if (cancelled) return;
          setHealth(nextHealth);
          persistedSettingsRef.current = nextSettings;
          setSettings(nextSettings);
          setAsrCapabilities(nextAsrCapabilities);
          clearErrorFrom("core");
        } catch (reason) {
          if (cancelled) return;
          reportError(reason, "errors.core.connect", "core");
        }
      } else {
        try {
          const nextHealth = await coreApi.health();
          if (!cancelled) setHealth(nextHealth);
        } catch {
          if (!cancelled) setHealth(null);
        }
      }
      if (!cancelled) timer = window.setTimeout(() => void poll(), 2500);
    };
    void poll();
    return () => {
      cancelled = true;
      if (timer !== null) window.clearTimeout(timer);
    };
  }, [clearErrorFrom, coreConfigured, reportError, settings]);

  const loadDevices = useCallback(async () => {
    if (!coreConfigured) return;
    try {
      setDevices(await coreApi.devices());
      setDevicesReady(true);
      clearErrorFrom("devices");
    } catch (reason) {
      setDevicesReady(false);
      reportError(reason, "errors.audio.devices", "devices");
    }
  }, [clearErrorFrom, coreConfigured, reportError]);

  const loadDictionaries = useCallback(async () => {
    if (!coreConfigured) return;
    try {
      setDictionarySources(await coreApi.dictionaries());
      clearErrorFrom("dictionary");
    } catch (reason) {
      reportError(reason, "errors.dictionary.list", "dictionary");
    }
  }, [clearErrorFrom, coreConfigured, reportError]);

  const loadAsrCapabilities = useCallback(async () => {
    if (!coreConfigured) return;
    try {
      setAsrCapabilities(await coreApi.asrCapabilities());
      clearErrorFrom("asr");
    } catch (reason) {
      reportError(reason, "errors.asr.capabilities", "asr");
    }
  }, [clearErrorFrom, coreConfigured, reportError]);
  const loadAsrCapabilitiesRef = useRef(loadAsrCapabilities);
  loadAsrCapabilitiesRef.current = loadAsrCapabilities;

  useEffect(() => {
    if (settingsPageActive) {
      void Promise.all([loadDevices(), loadDictionaries(), loadAsrCapabilities()]);
    }
  }, [
    loadAsrCapabilities,
    loadDevices,
    loadDictionaries,
    settingsPageActive,
  ]);

  const toggleCapture = useCallback(async () => {
    if (capturePendingRef.current) return;
    capturePendingRef.current = true;
    setCapturePending(true);
    try {
      if (healthRef.current?.capture_requested) {
        await coreApi.stop();
        clearPartials();
      }
      else await coreApi.start();
      clearErrorFrom("capture");
    } finally {
      try {
        setHealth(await coreApi.health());
      } catch {
        // Preserve the original capture error when the follow-up refresh also fails.
      } finally {
        capturePendingRef.current = false;
        setCapturePending(false);
      }
    }
  }, [clearErrorFrom, clearPartials]);

  const startMicrophoneTest = useCallback(async () => {
    try {
      const result = await coreApi.startMicrophoneTest();
      setHealth((current) => current ? {
        ...current,
        microphone_test_running: result.running,
        microphone_test_device: result.device,
      } : current);
      clearErrorFrom("microphone-test");
      void coreApi.health().then(setHealth).catch(() => undefined);
    } catch (reason) {
      reportError(reason, "errors.audio.microphoneTestFailed", "microphone-test");
      throw reason;
    }
  }, [clearErrorFrom, reportError]);

  const stopMicrophoneTest = useCallback(async () => {
    try {
      const result = await coreApi.stopMicrophoneTest();
      clearPartials();
      setHealth((current) => current ? {
        ...current,
        microphone_test_running: result.running,
        microphone_test_device: null,
      } : current);
      clearErrorFrom("microphone-test");
      void coreApi.health().then(setHealth).catch(() => undefined);
    } catch (reason) {
      reportError(reason, "errors.audio.microphoneTestFailed", "microphone-test");
      throw reason;
    }
  }, [clearErrorFrom, clearPartials, reportError]);

  const testOsc = useCallback(async () => {
    await coreApi.testOsc();
    clearErrorFrom("osc");
    window.setTimeout(() => {
      void coreApi.health().then(setHealth).catch(() => undefined);
    }, 200);
  }, [clearErrorFrom]);

  const persistSettings = useCallback(async (next: Settings): Promise<Settings> => {
    const previous = persistedSettingsRef.current;
    const stopMicrophoneTestForSettings = Boolean(
      healthRef.current?.microphone_test_running
      && previous !== null
      && (
        previous.audio.microphone.mode !== next.audio.microphone.mode
        || previous.audio.microphone.device_id !== next.audio.microphone.device_id
      )
    );
    if (stopMicrophoneTestForSettings) {
      await coreApi.stopMicrophoneTest();
      clearPartials();
      setHealth(await coreApi.health());
    }
    const saved = await coreApi.saveSettings(next);
    persistedSettingsRef.current = saved;
    void coreApi.health().then(setHealth).catch(() => undefined);
    return saved;
  }, [clearPartials]);
  const persistSettingsRef = useRef(persistSettings);
  persistSettingsRef.current = persistSettings;
  const settingsAutosaveRef = useRef<
    ReturnType<typeof createSettingsAutosave<Settings>> | null
  >(null);
  if (settingsAutosaveRef.current === null) {
    settingsAutosaveRef.current = createSettingsAutosave<Settings>({
      persist: (next) => persistSettingsRef.current(next),
      onOptimistic: setSettings,
      onCommit: () => {
        clearErrorFrom("settings");
        void loadAsrCapabilitiesRef.current();
      },
      onError: (reason) => {
        if (persistedSettingsRef.current) {
          setSettings(persistedSettingsRef.current);
        }
        reportError(reason, "errors.settings.apply", "settings");
        void loadSettingsRef.current();
      },
    });
  }

  const importDictionary = useCallback(async (
    file: File,
    onProgress?: (progress: number) => void,
  ) => {
    const imported = await coreApi.importDictionary(file, onProgress);
    await loadDictionaries();
    return imported;
  }, [loadDictionaries]);

  const deleteDictionary = useCallback(async (id: number) => {
    await coreApi.deleteDictionary(id);
    await loadDictionaries();
  }, [loadDictionaries]);

  return {
    connection,
    coreReady: startupState === "ready",
    startupFailed: startupState === "failed",
    health,
    capturePending,
    openedConversationId,
    subtitles,
    hasOlderSubtitles,
    loadingConversationSubtitles,
    loadingOlderSubtitles,
    conversationCatalogEvent,
    openConversation,
    loadOlderSubtitles,
    vrchatMuteStatus: vrchatMuteStatus ?? health?.vrchat_mute_sync ?? null,
    settings,
    devices,
    devicesReady,
    asrCapabilities,
    dictionarySources,
    error,
    clearError,
    clearErrorFrom,
    reportError,
    retryCore,
    loadSettings,
    loadDevices,
    loadAsrCapabilities,
    toggleCapture,
    startMicrophoneTest,
    stopMicrophoneTest,
    testOsc,
    saveSettings: settingsAutosaveRef.current,
    importDictionary,
    deleteDictionary,
    translateSubtitle,
    translatingSubtitleIds,
  };
}
