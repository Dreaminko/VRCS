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
  healthRef.current = health;

  const replaceHealth = useCallback((next: Health | null) => setHealth(next), []);
  const patchHealth = useCallback((update: (current: Health | null) => Health | null) => {
    setHealth(update);
  }, []);
  const getCurrentHealth = useCallback(() => healthRef.current, []);
  const refreshHealthQuietly = useCallback(async () => {
    try {
      const next = await runtimeApi.health();
      setHealth(next);
      return next;
    } catch {
      return null;
    }
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
      try {
        const next = await runtimeApi.health();
        if (cancelled) return;
        setHealth(next);
        clearErrorFrom("core");
      } catch (reason) {
        if (cancelled) return;
        if (healthRef.current === null) reportError(reason, "errors.core.connect", "core");
        else setHealth(null);
      }
      if (!cancelled) timer = window.setTimeout(() => void poll(), 2500);
    };
    void poll();
    return () => {
      cancelled = true;
      if (timer !== null) window.clearTimeout(timer);
    };
  }, [clearErrorFrom, coreConfigured, reportError]);

  const retry = useCallback(async () => {
    clearError();
    setCoreConfigured(false);
    setStartupState("starting");
    setHealth(null);
    try {
      await retryCoreStartup();
      setStartupAttempt((attempt) => attempt + 1);
    } catch (reason) {
      setStartupState("failed");
      reportError(reason, "errors.core.initialize", "core");
    }
  }, [clearError, reportError]);

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
