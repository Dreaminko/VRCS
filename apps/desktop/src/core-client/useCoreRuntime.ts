import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { publishHealth } from "./health-store";
import { runtimeApi } from "./runtime-api";
import {
  coreStartup,
  initializeCoreApi,
  retryCore as retryCoreStartup,
} from "./startup-api";
import type { Health } from "./types";
import type { PlatformState } from "./capabilities";
import type { ReportRuntimeError } from "./useRuntimeErrors";

export interface CoreHealthController {
  value: Health | null;
  getCurrent: () => Health | null;
  replace: (health: Health | null) => void;
  patch: (update: (current: Health | null) => Health | null) => void;
  refreshQuietly: () => Promise<Health | null>;
}

function shellHealthKey(health: Health | null): string | null {
  if (!health) return null;
  return JSON.stringify({
    status: health.status,
    config_schema: health.config_schema,
    capture_running: health.capture_running,
    capture_requested: health.capture_requested,
    microphone_capture_state: health.microphone_capture_state,
    audio_device: health.audio_device,
    microphone_device: health.microphone_device,
    microphone_test_running: health.microphone_test_running,
    microphone_test_device: health.microphone_test_device,
    asr_status: health.asr_status,
    vad_backend: health.vad_backend,
    vad_model_version: health.vad_model_version,
    last_error: health.last_error,
    osc: health.osc,
    vrchat_mute_sync: health.vrchat_mute_sync,
    language_session: health.language_session,
    room_scope_id: health.room_context?.scope_id ?? null,
    player_audio: health.player_audio
      ? {
          phase: health.player_audio.phase,
          active_players: health.player_audio.active_players,
          detail: health.player_audio.detail,
          key_scan: health.player_audio.key_scan ?? null,
        }
      : null,
    vrchat_chatbox: health.vrchat_chatbox
      ? {
          phase: health.vrchat_chatbox.phase,
          detail: health.vrchat_chatbox.detail,
        }
      : null,
  });
}

export function useCoreRuntime({
  clearError,
  clearErrorFrom,
  reportError,
  reportMessage,
}: {
  clearError: () => void;
  clearErrorFrom: (source: string) => void;
  reportError: ReportRuntimeError;
  reportMessage: (source: string, message: string) => void;
}) {
  const { t } = useTranslation();
  const tRef = useRef(t);
  tRef.current = t;
  const [coreConfigured, setCoreConfigured] = useState(false);
  const [startupState, setStartupState] = useState<"starting" | "ready" | "failed">("starting");
  const [startupAttempt, setStartupAttempt] = useState(0);
  const [health, setHealth] = useState<Health | null>(null);
  const [capabilities, setCapabilities] = useState<PlatformState>({ core: null, desktop: null });
  const healthRef = useRef<Health | null>(null);
  const capabilitiesJsonRef = useRef<string | null>(null);
  const healthJsonRef = useRef<string | null>(null);
  const shellHealthJsonRef = useRef<string | null>(null);

  const commitCapabilities = useCallback((next: PlatformState) => {
    const serialized = JSON.stringify(next);
    if (serialized === capabilitiesJsonRef.current) return;
    capabilitiesJsonRef.current = serialized;
    setCapabilities(next);
  }, []);

  const refreshCapabilities = useCallback(async (isCancelled?: () => boolean) => {
    const [coreResult, desktopResult] = await Promise.allSettled([
      runtimeApi.capabilities(),
      runtimeApi.platformCapabilities(),
    ]);
    if (isCancelled?.()) return;
    setCapabilities((current) => {
      const next: PlatformState = {
        core: coreResult.status === "fulfilled" ? coreResult.value : current.core,
        desktop: desktopResult.status === "fulfilled" ? desktopResult.value : current.desktop,
      };
      const serialized = JSON.stringify(next);
      if (serialized === capabilitiesJsonRef.current) return current;
      capabilitiesJsonRef.current = serialized;
      return next;
    });
  }, []);

  const commitHealth = useCallback((next: Health | null) => {
    const serialized = next ? JSON.stringify(next) : null;
    if (serialized === healthJsonRef.current) return;
    healthJsonRef.current = serialized;
    healthRef.current = next;
    publishHealth(next);
    const shellKey = shellHealthKey(next);
    if (shellKey !== shellHealthJsonRef.current) {
      shellHealthJsonRef.current = shellKey;
      setHealth(next);
    }
  }, []);

  const replaceHealth = useCallback((next: Health | null) => {
    commitHealth(next);
  }, [commitHealth]);
  const patchHealth = useCallback((update: (current: Health | null) => Health | null) => {
    commitHealth(update(healthRef.current));
  }, [commitHealth]);
  const getCurrentHealth = useCallback(() => healthRef.current, []);
  const refreshHealthQuietly = useCallback(async () => {
    try {
      const next = await runtimeApi.health();
      commitHealth(next);
      return healthRef.current;
    } catch {
      return null;
    }
  }, [commitHealth]);

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
          reportMessage("core", tRef.current("errors.core.initialize"));
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
  }, [reportError, reportMessage, startupAttempt]);

  useEffect(() => {
    if (!coreConfigured) return;
    let cancelled = false;
    let unlisten: UnlistenFn | null = null;
    void refreshCapabilities(() => cancelled);
    if (isTauri()) {
      void listen("desktop-shortcuts-changed", async () => {
        try {
          const desktop = await runtimeApi.platformCapabilities();
          if (cancelled) return;
          setCapabilities((current) => {
            const next: PlatformState = { core: current.core, desktop };
            const serialized = JSON.stringify(next);
            if (serialized === capabilitiesJsonRef.current) return current;
            capabilitiesJsonRef.current = serialized;
            return next;
          });
        } catch {
          // Leave existing desktop capabilities untouched on transient query failure.
        }
      }).then((dispose) => {
        if (cancelled) dispose();
        else unlisten = dispose;
      });
    }
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [coreConfigured, refreshCapabilities]);

  useEffect(() => {
    if (!coreConfigured) return;
    let cancelled = false;
    let timer: number | null = null;
    const poll = async () => {
      let interval = 2500;
      try {
        const next = await runtimeApi.health();
        if (cancelled) return;
        if (next.capture_requested && ["waiting_key", "acquiring_key", "retrying_key"].includes(next.player_audio?.phase ?? "")) interval = 250;
        commitHealth(next);
        clearErrorFrom("core");
      } catch (reason) {
        if (cancelled) return;
        if (healthRef.current === null) reportError(reason, "errors.core.connect", "core");
        else commitHealth(null);
      }
      if (!cancelled) timer = window.setTimeout(() => void poll(), interval);
    };
    void poll();
    return () => {
      cancelled = true;
      if (timer !== null) window.clearTimeout(timer);
    };
  }, [clearErrorFrom, commitHealth, coreConfigured, reportError]);

  const retry = useCallback(async () => {
    clearError();
    setCoreConfigured(false);
    setStartupState("starting");
    commitHealth(null);
    commitCapabilities({ core: null, desktop: null });
    try {
      await retryCoreStartup();
      setStartupAttempt((attempt) => attempt + 1);
    } catch (reason) {
      setStartupState("failed");
      reportError(reason, "errors.core.initialize", "core");
    }
  }, [clearError, commitCapabilities, commitHealth, reportError]);

  const healthController: CoreHealthController = {
    value: health,
    getCurrent: getCurrentHealth,
    replace: replaceHealth,
    patch: patchHealth,
    refreshQuietly: refreshHealthQuietly,
  };

  return {
    coreConfigured,
    ready: startupState === "ready",
    startupFailed: startupState === "failed",
    retry,
    health: healthController,
    capabilities,
  };
}
