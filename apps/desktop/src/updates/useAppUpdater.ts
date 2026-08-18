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
  loadAutomaticUpdatePreference,
  saveAutomaticUpdatePreference,
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
}

export function useAppUpdater(enabled: boolean): AppUpdaterState {
  const [buildInfo, setBuildInfo] = useState<AppBuildInfo | null>(null);
  const [automaticChecks, setAutomaticChecksState] = useState(true);
  const [preferenceReady, setPreferenceReady] = useState(false);
  const [preferenceSaving, setPreferenceSaving] = useState(false);
  const [phase, setPhase] = useState<UpdatePhase>("idle");
  const [update, setUpdate] = useState<AppUpdateMetadata | null>(null);
  const [errorCode, setErrorCode] = useState<string | null>(null);
  const [downloadedBytes, setDownloadedBytes] = useState(0);
  const [totalBytes, setTotalBytes] = useState<number | null>(null);
  const [noticeVisible, setNoticeVisible] = useState(false);
  const busyRef = useRef(false);
  const preferenceSavingRef = useRef(false);

  useEffect(() => {
    let cancelled = false;
    void loadAppBuildInfo().then((info) => {
      if (!cancelled) setBuildInfo(info);
    }).catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    void loadAutomaticUpdatePreference().then((automatic) => {
      if (!cancelled) setAutomaticChecksState(automatic);
    }).catch(() => undefined).finally(() => {
      if (!cancelled) setPreferenceReady(true);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const check = useCallback(async (manual = true) => {
    if (busyRef.current || buildInfo?.updaterAvailable === false) return;
    busyRef.current = true;
    setPhase("checking");
    setErrorCode(null);
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
    setDownloadedBytes(0);
    setTotalBytes(null);
    try {
      await downloadAndInstallAppUpdate((event) => {
        if (event.event === "started") {
          setTotalBytes(event.data.contentLength);
        } else if (event.event === "progress") {
          setDownloadedBytes((current) => current + event.data.chunkLength);
        } else {
          setPhase("installing");
        }
      });
    } catch (reason) {
      setErrorCode(updaterErrorCode(reason));
      setPhase("error");
      busyRef.current = false;
    }
  }, [update]);

  const setAutomaticChecks = useCallback(async (next: boolean) => {
    if (preferenceSavingRef.current) return;
    const previous = automaticChecks;
    preferenceSavingRef.current = true;
    setPreferenceSaving(true);
    setAutomaticChecksState(next);
    try {
      await saveAutomaticUpdatePreference(next);
    } catch {
      setAutomaticChecksState(previous);
      setErrorCode("failed");
      setPhase("error");
    } finally {
      preferenceSavingRef.current = false;
      setPreferenceSaving(false);
    }
  }, [automaticChecks]);

  return {
    buildInfo,
    automaticChecks,
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
  };
}
