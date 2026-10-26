import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";

import { PLAYER_AUDIO_READING_MS, type PlayerAudioPreparation } from "./player-audio-vr";
export { PLAYER_AUDIO_READING_MS } from "./player-audio-vr";

type ElevationAction = "start_capture" | "inspect_memory_compression" | "disable_memory_compression" | "enable_memory_compression";
export interface MemoryCompressionStatus {
  supported: boolean;
  elevated: boolean;
  enabled: boolean | null;
  restart_required: boolean;
}
type MemoryDialog = "manage" | "disable" | "enable" | null;

export function usePlayerAudioStart(options?: { beforeMemoryChange: () => Promise<boolean> }) {
  const { t } = useTranslation();
  const optionsRef = useRef(options);
  optionsRef.current = options;
  const [notice, setNotice] = useState<{ requestId: number; openedAt: number } | null>(null);
  const sequence = useRef(0);
  const pending = useRef<{ requestId: number; openedAt: number; promise: Promise<boolean>; resolve: (value: boolean) => void } | null>(null);
  const [preparation, setPreparation] = useState<PlayerAudioPreparation | null>(null);
  const [resumeRequested, setResumeRequested] = useState(false);
  const resumeApproved = useRef(false);
  const [memoryStatus, setMemoryStatus] = useState<MemoryCompressionStatus | null>(null);
  const [memoryDialog, setMemoryDialog] = useState<MemoryDialog>(null);
  const [memoryError, setMemoryError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [changing, setChanging] = useState(false);
  const refreshPending = useRef<Promise<MemoryCompressionStatus> | null>(null);
  const changePending = useRef(false);

  const refreshMemory = useCallback((): Promise<MemoryCompressionStatus> => {
    if (refreshPending.current) return refreshPending.current;
    setRefreshing(true);
    setMemoryError(null);
    const request = (isTauri()
      ? invoke<MemoryCompressionStatus>("player_audio_memory_status")
      : Promise.resolve({ supported: false, elevated: false, enabled: null, restart_required: false }))
      .then((status) => { setMemoryStatus(status); return status; })
      .catch((error) => { setMemoryError(String(error)); throw error; })
      .finally(() => { refreshPending.current = null; setRefreshing(false); });
    refreshPending.current = request;
    return request;
  }, []);

  const elevate = useCallback(async (action: ElevationAction) => {
    if (!isTauri()) return true;
    const result = await invoke<"ready" | "relaunching" | "cancelled">("player_audio_ensure_privileges", { action });
    if (result === "cancelled") throw new Error(t("warnings.playerAudio.elevationCancelled"));
    return result === "ready";
  }, [t]);

  const runMemoryOperation = useCallback(async (operation: () => Promise<void>) => {
    if (changePending.current) return;
    changePending.current = true;
    setChanging(true);
    setMemoryError(null);
    try {
      await refreshPending.current?.catch(() => undefined);
      await operation();
    } catch (error) { setMemoryError(String(error)); }
    finally { changePending.current = false; setChanging(false); }
  }, []);

  const changeMemory = useCallback((enabled: boolean, resumed = false) => runMemoryOperation(async () => {
    if (!isTauri()) throw new Error(t("settings.audio.memory.desktopOnly"));
    if (!resumed && !await elevate(enabled ? "enable_memory_compression" : "disable_memory_compression")) return;
    if (optionsRef.current && !await optionsRef.current.beforeMemoryChange()) {
      throw new Error(t("settings.audio.memory.pauseFailed"));
    }
    const status = await invoke<MemoryCompressionStatus>("player_audio_set_memory_compression", { enabled });
    setMemoryStatus(status);
    setMemoryDialog("manage");
  }), [elevate, runMemoryOperation, t]);

  const authorizeMemory = useCallback(() => runMemoryOperation(async () => {
    if (await elevate("inspect_memory_compression")) await refreshMemory();
  }), [elevate, refreshMemory, runMemoryOperation]);

  useEffect(() => {
    let active = true;
    void refreshMemory().catch(() => undefined);
    if (isTauri()) void invoke<ElevationAction | null>("player_audio_pending_action").then(async (action) => {
      if (!active || !action) return;
      if (action === "start_capture") {
        resumeApproved.current = true;
        setResumeRequested(true);
      } else {
        const accepted = await invoke<ElevationAction | null>("player_audio_consume_action");
        if (accepted !== action) return;
        setMemoryDialog("manage");
        if (action === "inspect_memory_compression") await refreshMemory();
        else await changeMemory(action === "enable_memory_compression", true);
      }
    }).catch((error) => { if (active) setMemoryError(String(error)); });
    return () => { active = false; };
  }, [refreshMemory, changeMemory]);
  useEffect(() => () => { pending.current?.resolve(false); pending.current = null; }, []);

  const confirm = useCallback(() => {
    if (pending.current) return pending.current.promise;
    const openedAt = performance.now();
    let resolve!: (value: boolean) => void;
    const promise = new Promise<boolean>((done) => { resolve = done; });
    const requestId = ++sequence.current;
    pending.current = { requestId, openedAt, promise, resolve };
    setPreparation(null);
    setNotice({ requestId, openedAt });
    return promise;
  }, []);

  const decide = useCallback((accepted: boolean, requestId?: number) => {
    const request = pending.current;
    if (!request || requestId !== undefined && requestId !== request.requestId
      || performance.now() - request.openedAt < PLAYER_AUDIO_READING_MS) return;
    pending.current = null;
    setNotice(null);
    request.resolve(accepted);
  }, []);

  const checkMemoryBeforeStart = useCallback(async () => {
    if (!isTauri()) return true;
    setPreparation({ stage: "memory", detail: null });
    try {
      const status = await refreshMemory();
      if (status.supported && status.enabled === false && !status.restart_required) {
        setPreparation(null);
        return true;
      }
      const key = !status.supported ? "desktopOnly" : status.restart_required ? "restart" : status.enabled ? "enabled" : "unknown";
      setPreparation({ stage: "blocked", detail: t(`settings.audio.memory.${key}`) });
    } catch (error) { setPreparation({ stage: "failed", detail: String(error) }); }
    setMemoryDialog("manage");
    return false;
  }, [refreshMemory, t]);

  const ensurePrivileges = useCallback(async () => {
    setPreparation({ stage: "privileges", detail: null });
    try {
      if (!await elevate("start_capture")) {
        setPreparation({ stage: "relaunching", detail: null });
        return false;
      }
      const ready = await checkMemoryBeforeStart();
      if (ready) setPreparation(null);
      return ready;
    } catch (error) {
      setPreparation({ stage: "failed", detail: String(error) });
      throw error;
    }
  }, [elevate, checkMemoryBeforeStart]);

  useEffect(() => {
    if (preparation?.stage !== "blocked" && preparation?.stage !== "failed") return;
    const timer = window.setTimeout(() => setPreparation(null), 10_000);
    return () => window.clearTimeout(timer);
  }, [preparation]);

  const prepare = useCallback(async () => {
    if (resumeApproved.current) {
      resumeApproved.current = false;
      // Consumed only after Core and settings are ready, not during React's mount probe.
      return isTauri() && await invoke<ElevationAction | null>("player_audio_consume_action") === "start_capture"
        && await checkMemoryBeforeStart();
    }
    return await confirm() && await ensurePrivileges();
  }, [confirm, ensurePrivileges, checkMemoryBeforeStart]);

  return { notice, preparation, decide, confirm, ensurePrivileges, prepare, resumeRequested, memory: {
    status: memoryStatus, dialog: memoryDialog, error: memoryError, busy: refreshing || changing,
    changing, refresh: () => { void refreshMemory().catch(() => undefined); },
    open: () => { setMemoryDialog("manage"); void refreshMemory().catch(() => undefined); },
    close: () => { if (!changePending.current) setMemoryDialog(null); },
    requestChange: (enabled: boolean) => { setMemoryError(null); setMemoryDialog(enabled ? "enable" : "disable"); },
    cancelChange: () => { if (!changePending.current) setMemoryDialog("manage"); },
    confirmChange: () => { if (memoryDialog === "enable" || memoryDialog === "disable") void changeMemory(memoryDialog === "enable"); },
    authorize: () => { void authorizeMemory(); },
  } };
}

export type PlayerAudioStart = ReturnType<typeof usePlayerAudioStart>;
