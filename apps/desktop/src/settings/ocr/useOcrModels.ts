import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { request } from "../../core-client/transport";
import type { VrOcrModelStatus } from "../../integrations/types";

export function useOcrModels() {
  const { t } = useTranslation();
  const [models, setModels] = useState<VrOcrModelStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [error, setError] = useState("");

  const load = useCallback(async (signal?: AbortSignal) => {
    try {
      const next = await request<VrOcrModelStatus>("/api/ocr/models", { signal });
      setModels(next);
      setError("");
    } catch {
      setError(t("settings.vrOcr.modelFailed"));
    }
  }, [t]);

  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const controller = new AbortController();
    const poll = async () => {
      try {
        const next = await request<VrOcrModelStatus>("/api/ocr/models", { signal: controller.signal });
        if (disposed) return;
        setModels(next);
        setError("");
        if (next.state === "downloading") timer = setTimeout(() => void poll(), 1_000);
      } catch {
        if (disposed) return;
        setError(t("settings.vrOcr.modelFailed"));
        timer = setTimeout(() => void poll(), 3_000);
      }
    };
    void poll();
    return () => {
      disposed = true;
      if (timer) clearTimeout(timer);
      controller.abort();
    };
  }, [models?.state, t]);

  const download = async () => {
    if (busy) return false;
    setBusy(true);
    setError("");
    try {
      setModels(await request<VrOcrModelStatus>("/api/ocr/models/download", { method: "POST" }));
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
    setRemoving(true);
    setError("");
    try {
      setModels(await request<VrOcrModelStatus>("/api/ocr/models", { method: "DELETE" }));
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
    refresh: load,
    download,
    remove,
  };
}
