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
  LiveTranscription,
  Settings,
  Subtitle,
  SubtitleTranslation,
} from "../types";

function withTranslation(
  subtitles: Subtitle[],
  subtitleId: number,
  translation: SubtitleTranslation,
): Subtitle[] {
  return subtitles.map((subtitle) => subtitle.id === subtitleId
    ? {
        ...subtitle,
        translations: [
          ...subtitle.translations.filter(
            (item) => item.target_language !== translation.target_language,
          ),
          translation,
        ],
        translation_partial: undefined,
      }
    : subtitle);
}

function withTranslationPartial(
  subtitles: Subtitle[],
  subtitleId: number,
  text?: string,
  targetLanguage?: string,
): Subtitle[] {
  return subtitles.map((subtitle) => subtitle.id === subtitleId
    ? {
        ...subtitle,
        translation_partial: text && targetLanguage
          ? { text, target_language: targetLanguage }
          : undefined,
      }
    : subtitle);
}

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
  const [partials, setPartials] = useState<Partial<Record<LiveTranscription["source"], LiveTranscription>>>({});
  const [settings, setSettings] = useState<Settings | null>(null);
  const persistedSettingsRef = useRef<Settings | null>(null);
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [devicesReady, setDevicesReady] = useState(false);
  const [asrCapabilities, setAsrCapabilities] = useState<AsrCapabilities | null>(null);
  const [dictionarySources, setDictionarySources] = useState<DictionarySource[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [translatingSubtitleIds, setTranslatingSubtitleIds] = useState<number[]>([]);

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
          setError(tRef.current("errors.core.initialize"));
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
        const message = JSON.parse(String(event.data)) as {
          type: string;
          subtitle?: Subtitle;
          source?: LiveTranscription["source"];
          text?: string;
          utterance_id?: string;
          language?: string | null;
          detail?: string;
          code?: string;
          subtitle_id?: number;
          translation?: SubtitleTranslation;
          target_language?: string;
        };
        if (message.type === "subtitle" && message.subtitle) {
          setSubtitles((current) => [message.subtitle!, ...current].slice(0, 500));
          const source = message.subtitle.source ?? "speaker";
          setPartials((current) => ({ ...current, [source]: undefined }));
        } else if (message.type === "partial" && message.source && message.text && message.utterance_id) {
          const partial = message as LiveTranscription;
          setPartials((current) => ({ ...current, [partial.source]: partial }));
        } else if (message.type === "failed" && message.detail) {
          setError(localizedError(
            { code: message.code ?? "asr.cloud_connect_failed" },
            tRef.current,
            "errors.core.connect",
          ));
        } else if (message.type === "translation_started" && message.subtitle_id !== undefined) {
          setSubtitles((current) => withTranslationPartial(current, message.subtitle_id!));
          setTranslatingSubtitleIds((current) => current.includes(message.subtitle_id!)
            ? current
            : [...current, message.subtitle_id!]);
        } else if (
          message.type === "translation_partial"
          && message.subtitle_id !== undefined
          && message.text
          && message.target_language
        ) {
          setSubtitles((current) => withTranslationPartial(
            current,
            message.subtitle_id!,
            message.text,
            message.target_language,
          ));
        } else if (
          message.type === "translation_completed"
          && message.subtitle_id !== undefined
          && message.translation
        ) {
          setSubtitles((current) => withTranslation(
            current,
            message.subtitle_id!,
            message.translation!,
          ));
          setTranslatingSubtitleIds((current) => current.filter((id) => id !== message.subtitle_id));
        } else if (message.type === "translation_failed" && message.subtitle_id !== undefined) {
          setSubtitles((current) => withTranslationPartial(current, message.subtitle_id!));
          setTranslatingSubtitleIds((current) => current.filter((id) => id !== message.subtitle_id));
          setError(localizedError(
            { code: message.code ?? "translation.request_failed" },
            tRef.current,
            "errors.translation.failed",
          ));
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
    if (healthRef.current?.capture_running) {
      await coreApi.stop();
      setPartials({});
    }
    else await coreApi.start();
    setHealth(await coreApi.health());
    clearError();
  }, [clearError]);

  const testOsc = useCallback(async () => {
    await coreApi.testOsc();
    clearError();
    window.setTimeout(() => {
      void coreApi.health().then(setHealth).catch(() => undefined);
    }, 200);
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

  const translateSubtitle = useCallback(async (subtitleId: number) => {
    setTranslatingSubtitleIds((current) => current.includes(subtitleId)
      ? current
      : [...current, subtitleId]);
    try {
      const translation = await coreApi.translateSubtitle(subtitleId);
      setSubtitles((current) => withTranslation(current, subtitleId, translation));
      clearError();
    } catch (reason) {
      setSubtitles((current) => withTranslationPartial(current, subtitleId));
      reportError(reason, "errors.translation.failed");
    } finally {
      setTranslatingSubtitleIds((current) => current.filter((id) => id !== subtitleId));
    }
  }, [clearError, reportError]);

  return {
    connection,
    coreReady: startupState === "ready",
    startupFailed: startupState === "failed",
    health,
    subtitles,
    partials,
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
    testOsc,
    saveSettings: settingsAutosaveRef.current,
    importDictionary,
    deleteDictionary,
    translateSubtitle,
    translatingSubtitleIds,
  };
}
