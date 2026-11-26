import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { request } from "../../core-client/transport";
import type { VrOcrModelStatus } from "../../integrations/types";

export function useOcrModels() {
  const { t } = useTranslation();
  const tRef = useRef(t);
  tRef.current = t;
  const [models, setModels] = useState<VrOcrModelStatus | null>(null);
  const stateRef = useRef<VrOcrModelStatus["state"] | undefined>(undefined);
  stateRef.current = models?.state;
  const [busy, setBusy] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [error, setError] = useState("");
  const schedulePollRef = useRef<(delayMs: number) => void>(() => undefined);
  const cancelPollRef = useRef<() => void>(() => undefined);

  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let controller: AbortController | null = null;

    const cancelPoll = () => {
      if (timer !== undefined) {
        clearTimeout(timer);
        timer = undefined;
      }
      controller?.abort();
      controller = null;
    };

    const schedule = (delayMs: number) => {
      if (disposed || timer !== undefined || document.hidden) return;
      timer = setTimeout(() => {
        timer = undefined;
        void poll();
      }, delayMs);
    };

    const poll = async () => {
      if (disposed || document.hidden) return;
      cancelPoll();
      const nextController = new AbortController();
      controller = nextController;
      try {
        const next = await request<VrOcrModelStatus>("/api/ocr/models", { signal: nextController.signal });
        if (disposed || nextController.signal.aborted) return;
        stateRef.current = next.state;
        setModels(next);
        setError("");
        if (next.state === "downloading") schedule(1_000);
      } catch {
        if (disposed || nextController.signal.aborted) return;
        setError(tRef.current("settings.vrOcr.modelFailed"));
        schedule(3_000);
      }
    };

    schedulePollRef.current = schedule;
    cancelPollRef.current = cancelPoll;
    const onVisibility = () => {
      if (!document.hidden && (stateRef.current === undefined || stateRef.current === "downloading")) {
        void poll();
      }
    };
    document.addEventListener("visibilitychange", onVisibility);
    void poll();
    return () => {
      disposed = true;
      schedulePollRef.current = () => undefined;
      cancelPollRef.current = () => undefined;
      cancelPoll();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, []);

  const download = async () => {
    if (busy) return false;
    cancelPollRef.current();
    setBusy(true);
    setError("");
    try {
      const next = await request<VrOcrModelStatus>("/api/ocr/models/download", { method: "POST" });
      stateRef.current = next.state;
      setModels(next);
      if (next.state === "downloading") schedulePollRef.current(1_000);
      return true;
    } catch {
      setError(t("settings.vrOcr.modelFailed"));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (removing) return false;
    cancelPollRef.current();
    setRemoving(true);
    setError("");
    try {
      const next = await request<VrOcrModelStatus>("/api/ocr/models", { method: "DELETE" });
      stateRef.current = next.state;
      setModels(next);
      return true;
    } catch {
      setError(t("settings.vrOcr.modelDeleteFailed"));
      return false;
    } finally {
      setRemoving(false);
    }
  };

  return {
    models,
    busy,
    removing,
    error,
    download,
    remove,
  };
}
