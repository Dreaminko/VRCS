import { useCallback, useEffect, useRef, useState } from "react";

import {
  checkForAppUpdate,
  downloadAndInstallAppUpdate,
  loadAppBuildInfo,
  updaterErrorCode,
  type AppBuildInfo,
  type AppUpdateMetadata,
} from "./app-updater";
import {
  loadUpdatePreferences,
  saveAutomaticUpdatePreference,
  saveNightlyUpdatePreference,
} from "./update-preferences";

const STARTUP_CHECK_DELAY_MS = 10_000;
const PERIODIC_CHECK_INTERVAL_MS = 24 * 60 * 60 * 1_000;

export type UpdatePhase =
  | "idle"
  | "checking"
  | "upToDate"
  | "available"
  | "downloading"
  | "installing"
  | "error";

export interface AppUpdaterState {
  buildInfo: AppBuildInfo | null;
  automaticChecks: boolean;
  receiveNightlyUpdates: boolean;
  preferenceReady: boolean;
  preferenceSaving: boolean;
  phase: UpdatePhase;
  update: AppUpdateMetadata | null;
  errorCode: string | null;
  downloadedBytes: number;
  totalBytes: number | null;
  noticeVisible: boolean;
  check: (manual?: boolean) => Promise<void>;
  install: () => Promise<void>;
  dismissNotice: () => void;
  setAutomaticChecks: (enabled: boolean) => Promise<void>;
  setReceiveNightlyUpdates: (enabled: boolean) => Promise<void>;
}

export function useAppUpdater(enabled: boolean): AppUpdaterState {
  const [buildInfo, setBuildInfo] = useState<AppBuildInfo | null>(null);
  const [automaticChecks, setAutomaticChecksState] = useState(true);
  const [receiveNightlyUpdates, setReceiveNightlyUpdatesState] = useState(false);
  const [preferenceReady, setPreferenceReady] = useState(false);
  const [preferenceSaving, setPreferenceSaving] = useState(false);
  const [phase, setPhase] = useState<UpdatePhase>("idle");
  const [update, setUpdate] = useState<AppUpdateMetadata | null>(null);
  const [errorCode, setErrorCode] = useState<string | null>(null);
  const [downloadedBytes, setDownloadedBytes] = useState(0);
  const [totalBytes, setTotalBytes] = useState<number | null>(null);
  const [noticeVisible, setNoticeVisible] = useState(false);
  const busyRef = useRef(false);
  const downloadedBytesRef = useRef(0);
  const progressFlushTimerRef = useRef<number | null>(null);

  useEffect(() => () => {
    if (progressFlushTimerRef.current !== null) window.clearTimeout(progressFlushTimerRef.current);
  }, []);

  useEffect(() => {
    let cancelled = false;
    void loadAppBuildInfo().then((info) => {
      if (!cancelled) setBuildInfo(info);
    }).catch((reason) => {
      console.warn("Failed to load app build info", reason);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    void loadUpdatePreferences().then((preferences) => {
      if (cancelled) return;
      setAutomaticChecksState(preferences.automaticChecks);
      setReceiveNightlyUpdatesState(preferences.receiveNightlyUpdates);
    }).catch((reason) => {
      console.warn("Failed to load update preferences", reason);
    }).finally(() => {
      if (!cancelled) setPreferenceReady(true);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const check = useCallback(async (manual = true) => {
    if (busyRef.current || !buildInfo?.updaterAvailable) return;
    busyRef.current = true;
    setPhase("checking");
    setErrorCode(null);
    setUpdate(null);
    setNoticeVisible(false);
    try {
      const available = await checkForAppUpdate();
      setUpdate(available);
      setPhase(available ? "available" : "upToDate");
      if (available) setNoticeVisible(true);
    } catch (reason) {
      if (manual) {
        setErrorCode(updaterErrorCode(reason));
        setPhase("error");
      } else {
        setPhase("idle");
      }
    } finally {
      busyRef.current = false;
    }
  }, [buildInfo?.updaterAvailable]);

  useEffect(() => {
    if (!enabled || !preferenceReady || !automaticChecks || !buildInfo?.updaterAvailable) return;
    const startupTimer = window.setTimeout(() => void check(false), STARTUP_CHECK_DELAY_MS);
    const periodicTimer = window.setInterval(() => void check(false), PERIODIC_CHECK_INTERVAL_MS);
    return () => {
      window.clearTimeout(startupTimer);
      window.clearInterval(periodicTimer);
    };
  }, [automaticChecks, buildInfo?.updaterAvailable, check, enabled, preferenceReady]);

  const install = useCallback(async () => {
    if (busyRef.current || !update) return;
    busyRef.current = true;
    setPhase("downloading");
    setErrorCode(null);
    downloadedBytesRef.current = 0;
    if (progressFlushTimerRef.current !== null) {
      window.clearTimeout(progressFlushTimerRef.current);
      progressFlushTimerRef.current = null;
    }
    setDownloadedBytes(0);
    setTotalBytes(null);
    const flushProgress = () => {
      if (progressFlushTimerRef.current !== null) {
        window.clearTimeout(progressFlushTimerRef.current);
        progressFlushTimerRef.current = null;
      }
      setDownloadedBytes(downloadedBytesRef.current);
    };
    try {
      await downloadAndInstallAppUpdate(update.version, (event) => {
        if (event.event === "started") {
          setTotalBytes(event.data.contentLength);
        } else if (event.event === "progress") {
          downloadedBytesRef.current += event.data.chunkLength;
          if (progressFlushTimerRef.current === null) {
            progressFlushTimerRef.current = window.setTimeout(() => {
              progressFlushTimerRef.current = null;
              setDownloadedBytes(downloadedBytesRef.current);
            }, 100);
          }
        } else {
          flushProgress();
          setPhase("installing");
        }
      });
      setPhase("idle");
    } catch (reason) {
      flushProgress();
      setErrorCode(updaterErrorCode(reason));
      setPhase("error");
    } finally {
      busyRef.current = false;
    }
  }, [update]);

  const setAutomaticChecks = useCallback(async (next: boolean) => {
    if (busyRef.current) return;
    const previous = automaticChecks;
    busyRef.current = true;
    setPreferenceSaving(true);
    setAutomaticChecksState(next);
    try {
      await saveAutomaticUpdatePreference(next);
    } catch (reason) {
      setAutomaticChecksState(previous);
      setErrorCode(updaterErrorCode(reason));
      setPhase("error");
    } finally {
      busyRef.current = false;
      setPreferenceSaving(false);
    }
  }, [automaticChecks]);

  const setReceiveNightlyUpdates = useCallback(async (next: boolean) => {
    if (busyRef.current) return;
    busyRef.current = true;
    setPreferenceSaving(true);
    try {
      await saveNightlyUpdatePreference(next);
      setReceiveNightlyUpdatesState(next);
      setUpdate(null);
      setNoticeVisible(false);
      setErrorCode(null);
      setPhase("idle");
    } catch (reason) {
      setErrorCode(updaterErrorCode(reason));
      setPhase("error");
      return;
    } finally {
      busyRef.current = false;
      setPreferenceSaving(false);
    }
    if (automaticChecks) await check(false);
  }, [automaticChecks, check]);

  return {
    buildInfo,
    automaticChecks,
    receiveNightlyUpdates,
    preferenceReady,
    preferenceSaving,
    phase,
    update,
    errorCode,
    downloadedBytes,
    totalBytes,
    noticeVisible,
    check,
    install,
    dismissNotice: () => setNoticeVisible(false),
    setAutomaticChecks,
    setReceiveNightlyUpdates,
  };
}
