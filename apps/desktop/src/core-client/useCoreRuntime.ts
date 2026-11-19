import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { runtimeApi } from "./runtime-api";
import {
  coreStartup,
  initializeCoreApi,
  retryCore as retryCoreStartup,
} from "./startup-api";
import type { Health } from "./types";
import type { ReportRuntimeError } from "./useRuntimeErrors";

export interface CoreHealthController {
  value: Health | null;
  getCurrent: () => Health | null;
  replace: (health: Health | null) => void;
  patch: (update: (current: Health | null) => Health | null) => void;
  refreshQuietly: () => Promise<Health | null>;
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
  const healthRef = useRef<Health | null>(null);
  const healthJsonRef = useRef<string | null>(null);

  const commitHealth = useCallback((next: Health | null) => {
    const serialized = next ? JSON.stringify(next) : null;
    if (serialized === healthJsonRef.current) return;
    healthJsonRef.current = serialized;
    healthRef.current = next;
    setHealth(next);
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
    try {
      await retryCoreStartup();
      setStartupAttempt((attempt) => attempt + 1);
    } catch (reason) {
      setStartupState("failed");
      reportError(reason, "errors.core.initialize", "core");
    }
  }, [clearError, commitHealth, reportError]);

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
  };
}
