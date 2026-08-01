import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  coreApi,
  coreStartup,
  coreWebSocketUrl,
  initializeCoreApi,
  retryCore as retryCoreStartup,
} from "../api";
import { localizedError } from "../app-utils";
import { createSettingsAutosave } from "../settings-autosave";
import { audioSettingsChanged } from "../settings-validation";
import type {
  AsrCapabilities,
  AudioDevice,
  ConnectionState,
  DictionarySource,
  Health,
  Settings,
  Subtitle,
} from "../types";

export function useCoreSession(settingsPageActive: boolean) {
  const { t } = useTranslation();
  const tRef = useRef(t);
  tRef.current = t;

  const [connection, setConnection] = useState<ConnectionState>("connecting");
  const [coreConfigured, setCoreConfigured] = useState(false);
  const [startupState, setStartupState] = useState<"starting" | "ready" | "failed">("starting");
  const [startupAttempt, setStartupAttempt] = useState(0);
  const [health, setHealth] = useState<Health | null>(null);
  const healthRef = useRef<Health | null>(null);
  healthRef.current = health;
  const [subtitles, setSubtitles] = useState<Subtitle[]>([]);
  const [settings, setSettings] = useState<Settings | null>(null);
  const persistedSettingsRef = useRef<Settings | null>(null);
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [devicesReady, setDevicesReady] = useState(false);
  const [asrCapabilities, setAsrCapabilities] = useState<AsrCapabilities | null>(null);
  const [dictionarySources, setDictionarySources] = useState<DictionarySource[]>([]);
  const [error, setError] = useState<string | null>(null);

  const reportError = useCallback((reason: unknown, fallbackKey: string) => {
    setError(localizedError(reason, tRef.current, fallbackKey));
  }, []);

  const clearError = useCallback(() => {
    setError(null);
  }, []);

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
          setConnection("disconnected");
          reportError(
            new Error(startup.error ?? "Core startup failed"),
            "errors.core.initialize",
          );
          return;
        }
        timer = window.setTimeout(() => void pollStartup(), 150);
      } catch (reason) {
        if (!cancelled) {
          setStartupState("failed");
          setConnection("disconnected");
          reportError(reason, "errors.core.initialize");
        }
      }
    };
    void initializeCoreApi().then(pollStartup).catch((reason) => {
      if (!cancelled) {
        setStartupState("failed");
        setConnection("disconnected");
        reportError(reason, "errors.core.initialize");
      }
    });
    return () => {
      cancelled = true;
      if (timer !== null) window.clearTimeout(timer);
    };
  }, [reportError, startupAttempt]);

  const retryCore = useCallback(async () => {
    clearError();
    setConnection("connecting");
    setCoreConfigured(false);
    setStartupState("starting");
    try {
      await retryCoreStartup();
      setStartupAttempt((attempt) => attempt + 1);
    } catch (reason) {
      setStartupState("failed");
      setConnection("disconnected");
      reportError(reason, "errors.core.initialize");
    }
  }, [clearError, reportError]);

  const refresh = useCallback(async () => {
    if (!coreConfigured) return;
    try {
      const [nextHealth, nextSettings, historyItems, nextAsrCapabilities] = await Promise.all([
        coreApi.health(),
        coreApi.settings(),
        coreApi.subtitles(),
        coreApi.asrCapabilities(),
      ]);
      setHealth(nextHealth);
      persistedSettingsRef.current = nextSettings;
      setSettings(nextSettings);
      setSubtitles(historyItems);
      setAsrCapabilities(nextAsrCapabilities);
      clearError();
    } catch (reason) {
      reportError(reason, "errors.core.connect");
    }
  }, [clearError, coreConfigured, reportError]);

  useEffect(() => {
    if (!coreConfigured) return;
    void refresh();
    const timer = window.setInterval(
      () => {
        if (settings === null) void refresh();
        else void coreApi.health().then(setHealth).catch(() => setHealth(null));
      },
      2500,
    );
    return () => window.clearInterval(timer);
  }, [coreConfigured, refresh, settings]);

  useEffect(() => {
    if (!coreConfigured) return;
    let socket: WebSocket | null = null;
    let retry: number | null = null;
    let closed = false;
    const connect = () => {
      setConnection("connecting");
      socket = new WebSocket(coreWebSocketUrl());
      socket.onopen = () => setConnection("connected");
      socket.onmessage = (event) => {
        const message = JSON.parse(String(event.data)) as { type: string; subtitle?: Subtitle };
        if (message.type === "subtitle" && message.subtitle) {
          setSubtitles((current) => [message.subtitle!, ...current].slice(0, 500));
        }
      };
      socket.onclose = () => {
        setConnection("disconnected");
        if (!closed) retry = window.setTimeout(connect, 1500);
      };
    };
    connect();
    return () => {
      closed = true;
      if (retry !== null) window.clearTimeout(retry);
      socket?.close();
    };
  }, [coreConfigured]);

  const loadDevices = useCallback(async () => {
    if (!coreConfigured) return;
    try {
      setDevices(await coreApi.devices());
      setDevicesReady(true);
      clearError();
    } catch (reason) {
      setDevicesReady(false);
      reportError(reason, "errors.audio.devices");
    }
  }, [clearError, coreConfigured, reportError]);

  const loadDictionaries = useCallback(async () => {
    if (!coreConfigured) return;
    try {
      setDictionarySources(await coreApi.dictionaries());
      clearError();
    } catch (reason) {
      reportError(reason, "errors.dictionary.list");
    }
  }, [clearError, coreConfigured, reportError]);

  const loadAsrCapabilities = useCallback(async () => {
    if (!coreConfigured) return;
    try {
      setAsrCapabilities(await coreApi.asrCapabilities());
      clearError();
    } catch (reason) {
      reportError(reason, "errors.asr.capabilities");
    }
  }, [clearError, coreConfigured, reportError]);
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
    if (healthRef.current?.capture_running) await coreApi.stop();
    else await coreApi.start();
    setHealth(await coreApi.health());
    clearError();
  }, [clearError]);

  const persistSettings = useCallback(async (next: Settings): Promise<Settings> => {
    const previous = persistedSettingsRef.current;
    const restartCapture = (
      Boolean(healthRef.current?.capture_running)
      && previous !== null
      && audioSettingsChanged(previous, next)
    );
    let captureStopped = false;
    let saved: Settings | null = null;

    try {
      if (restartCapture) {
        await coreApi.stop();
        captureStopped = true;
      }
      saved = await coreApi.saveSettings(next);
      if (restartCapture) {
        await coreApi.start();
        void coreApi.health().then(setHealth).catch(() => undefined);
      }
      persistedSettingsRef.current = saved;
      return saved;
    } catch (reason) {
      if (restartCapture && captureStopped) {
        let recoveryError: unknown = null;
        if (saved !== null && previous !== null) {
          try {
            await coreApi.saveSettings(previous);
          } catch (rollbackReason) {
            recoveryError = rollbackReason;
          }
        }
        try {
          await coreApi.start();
          void coreApi.health().then(setHealth).catch(() => undefined);
        } catch (restartReason) {
          recoveryError ??= restartReason;
        }
        if (recoveryError) {
          const applyMessage = localizedError(
            reason,
            tRef.current,
            "errors.settings.apply",
          );
          const recoveryMessage = localizedError(
            recoveryError,
            tRef.current,
            "errors.unknown",
          );
          throw new Error(
            tRef.current("errors.settings.recovery", {
              applyMessage,
              recoveryMessage,
            }),
            { cause: reason },
          );
        }
      }
      throw reason;
    }
  }, []);
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
        clearError();
        void loadAsrCapabilitiesRef.current();
      },
      onError: (reason) => {
        if (persistedSettingsRef.current) {
          setSettings(persistedSettingsRef.current);
        }
        reportError(reason, "errors.settings.apply");
      },
    });
  }

  const importDictionary = useCallback(async (file: File) => {
    const imported = await coreApi.importDictionary(file);
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
    subtitles,
    settings,
    devices,
    devicesReady,
    asrCapabilities,
    dictionarySources,
    error,
    clearError,
    reportError,
    retryCore,
    loadDevices,
    loadAsrCapabilities,
    toggleCapture,
    saveSettings: settingsAutosaveRef.current,
    importDictionary,
    deleteDictionary,
  };
}
