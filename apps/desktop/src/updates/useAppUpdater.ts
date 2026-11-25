import { useCallback, useEffect, useRef, useState } from "react";

import {
  checkForAppUpdate,
  downloadAndInstallAppUpdate,
  fetchGitHubReleaseNotes,
  loadAppBuildInfo,
  updaterErrorCode,
  type AppBuildInfo,
  type AppUpdateMetadata,
} from "./app-updater";
import {
  loadUpdatePreferences,
  saveAutomaticUpdatePreference,
  saveLastUpdateCheckAt,
  saveNightlyUpdatePreference,
  saveSkippedUpdateVersion,
} from "./update-preferences";

const STARTUP_CHECK_DELAY_MS = 10_000;
const PERIODIC_CHECK_INTERVAL_MS = 24 * 60 * 60 * 1_000;
const CHECK_ONLY_ERROR_CODES = new Set([
  "no_pending",
  "stable_check_failed",
  "nightly_check_failed",
  "both_checks_failed",
]);

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
  skippedVersion: string | null;
  lastCheckedAt: number | null;
  preferenceReady: boolean;
  preferenceSaving: boolean;
  phase: UpdatePhase;
  update: AppUpdateMetadata | null;
  liveNotes: string | null;
  liveNotesLoading: boolean;
  errorCode: string | null;
  downloadedBytes: number;
  totalBytes: number | null;
  downloadSpeedBytesPerSec: number | null;
  noticeVisible: boolean;
  detailsOpen: boolean;
  hasUnreadUpdate: boolean;
  check: (manual?: boolean) => Promise<void>;
  install: () => Promise<void>;
  retry: () => Promise<void>;
  openDetails: () => void;
  closeDetails: () => void;
  skipCurrentUpdate: () => Promise<void>;
  clearSkippedVersion: () => Promise<void>;
  dismissNotice: () => void;
  setAutomaticChecks: (enabled: boolean) => Promise<void>;
  setReceiveNightlyUpdates: (enabled: boolean) => Promise<void>;
}

export function useAppUpdater(enabled: boolean): AppUpdaterState {
  const [buildInfo, setBuildInfo] = useState<AppBuildInfo | null>(null);
  const [automaticChecks, setAutomaticChecksState] = useState(true);
  const [receiveNightlyUpdates, setReceiveNightlyUpdatesState] = useState(false);
  const [skippedVersion, setSkippedVersionState] = useState<string | null>(null);
  const [lastCheckedAt, setLastCheckedAt] = useState<number | null>(null);
  const [preferenceReady, setPreferenceReady] = useState(false);
  const [preferenceSaving, setPreferenceSaving] = useState(false);
  const [phase, setPhase] = useState<UpdatePhase>("idle");
  const [update, setUpdate] = useState<AppUpdateMetadata | null>(null);
  const [liveNotes, setLiveNotes] = useState<string | null>(null);
  const [liveNotesLoading, setLiveNotesLoading] = useState(false);
  const [errorCode, setErrorCode] = useState<string | null>(null);
  const [downloadedBytes, setDownloadedBytes] = useState(0);
  const [totalBytes, setTotalBytes] = useState<number | null>(null);
  const [downloadSpeedBytesPerSec, setDownloadSpeedBytesPerSec] = useState<number | null>(null);
  const [noticeVisible, setNoticeVisible] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(false);

  const busyRef = useRef(false);
  const updateRef = useRef<AppUpdateMetadata | null>(null);
  const phaseRef = useRef<UpdatePhase>("idle");
  const skippedVersionRef = useRef<string | null>(null);
  const downloadedBytesRef = useRef(0);
  const speedSampleRef = useRef<{ bytes: number; time: number }>({ bytes: 0, time: 0 });
  const progressFlushTimerRef = useRef<number | null>(null);
  const liveNotesCacheRef = useRef(new Map<string, string | null>());
  const liveNotesAbortRef = useRef<AbortController | null>(null);

  updateRef.current = update;
  phaseRef.current = phase;
  skippedVersionRef.current = skippedVersion;

  useEffect(() => () => {
    if (progressFlushTimerRef.current !== null) window.clearTimeout(progressFlushTimerRef.current);
    liveNotesAbortRef.current?.abort();
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
      setSkippedVersionState(preferences.skippedVersion);
      setLastCheckedAt(preferences.lastCheckedAt);
    }).catch((reason) => {
      console.warn("Failed to load update preferences", reason);
    }).finally(() => {
      if (!cancelled) setPreferenceReady(true);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const loadLiveNotes = useCallback((metadata: AppUpdateMetadata) => {
    liveNotesAbortRef.current?.abort();
    const tag = metadata.releaseTag?.trim();
    if (!tag) {
      setLiveNotes(null);
      setLiveNotesLoading(false);
      return;
    }
    if (liveNotesCacheRef.current.has(tag)) {
      setLiveNotes(liveNotesCacheRef.current.get(tag) ?? null);
      setLiveNotesLoading(false);
      return;
    }
    const controller = new AbortController();
    liveNotesAbortRef.current = controller;
    setLiveNotes(null);
    setLiveNotesLoading(true);
    void fetchGitHubReleaseNotes(tag, controller.signal)
      .then((notes) => {
        if (controller.signal.aborted) return;
        liveNotesCacheRef.current.set(tag, notes);
        setLiveNotes(notes);
      })
      .finally(() => {
        if (liveNotesAbortRef.current === controller) {
          liveNotesAbortRef.current = null;
          setLiveNotesLoading(false);
        }
      });
  }, []);

  const check = useCallback(async (manual = true) => {
    if (busyRef.current || !buildInfo?.updaterAvailable) return;
    busyRef.current = true;
    const previousUpdate = updateRef.current;
    const previousPhase = phaseRef.current;
    setPhase("checking");
    setErrorCode(null);
    if (manual) {
      setUpdate(null);
      setLiveNotes(null);
      setLiveNotesLoading(false);
      setNoticeVisible(false);
      setDetailsOpen(false);
    }
    try {
      const available = await checkForAppUpdate(!manual);
      const checkedAt = Date.now();
      setLastCheckedAt(checkedAt);
      void saveLastUpdateCheckAt(checkedAt).catch(() => undefined);

      if (available) {
        let activeSkipped = skippedVersionRef.current;
        if (activeSkipped && activeSkipped !== available.version) {
          activeSkipped = null;
          setSkippedVersionState(null);
          void saveSkippedUpdateVersion(null).catch(() => undefined);
        }
        setUpdate(available);
        setPhase("available");
        loadLiveNotes(available);
        if (manual || available.version !== activeSkipped) {
          setNoticeVisible(true);
        }
      } else {
        liveNotesAbortRef.current?.abort();
        setUpdate(null);
        setLiveNotes(null);
        setLiveNotesLoading(false);
        setNoticeVisible(false);
        setDetailsOpen(false);
        setPhase("upToDate");
      }
    } catch (reason) {
      if (manual) {
        setErrorCode(updaterErrorCode(reason));
        setPhase("error");
      } else {
        setPhase(previousUpdate ? (previousPhase === "available" ? "available" : "idle") : "idle");
      }
    } finally {
      busyRef.current = false;
    }
  }, [buildInfo?.updaterAvailable, loadLiveNotes]);

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
    const targetUpdate = updateRef.current;
    if (busyRef.current || !targetUpdate) return;
    busyRef.current = true;
    setPhase("downloading");
    setErrorCode(null);
    downloadedBytesRef.current = 0;
    const now = performance.now();
    speedSampleRef.current = { bytes: 0, time: now };
    if (progressFlushTimerRef.current !== null) {
      window.clearTimeout(progressFlushTimerRef.current);
      progressFlushTimerRef.current = null;
    }
    setDownloadedBytes(0);
    setTotalBytes(null);
    setDownloadSpeedBytesPerSec(null);

    const flushProgress = () => {
      if (progressFlushTimerRef.current !== null) {
        window.clearTimeout(progressFlushTimerRef.current);
        progressFlushTimerRef.current = null;
      }
      const currentTime = performance.now();
      const elapsedSeconds = (currentTime - speedSampleRef.current.time) / 1000;
      const deltaBytes = downloadedBytesRef.current - speedSampleRef.current.bytes;
      if (elapsedSeconds >= 0.4 && deltaBytes > 0) {
        setDownloadSpeedBytesPerSec(deltaBytes / elapsedSeconds);
        speedSampleRef.current = { bytes: downloadedBytesRef.current, time: currentTime };
      }
      setDownloadedBytes(downloadedBytesRef.current);
    };

    try {
      await downloadAndInstallAppUpdate(targetUpdate.version, (event) => {
        if (event.event === "started") {
          setTotalBytes(event.data.contentLength);
        } else if (event.event === "progress") {
          downloadedBytesRef.current += event.data.chunkLength;
          if (progressFlushTimerRef.current === null) {
            progressFlushTimerRef.current = window.setTimeout(flushProgress, 150);
          }
        } else {
          flushProgress();
          setDownloadSpeedBytesPerSec(null);
          setPhase("installing");
        }
      });
      setPhase("idle");
    } catch (reason) {
      flushProgress();
      setDownloadSpeedBytesPerSec(null);
      setErrorCode(updaterErrorCode(reason));
      setPhase("error");
    } finally {
      busyRef.current = false;
    }
  }, []);

  const retry = useCallback(async () => {
    if (!updateRef.current || (errorCode && CHECK_ONLY_ERROR_CODES.has(errorCode))) {
      await check(true);
      return;
    }
    await install();
  }, [check, errorCode, install]);

  const openDetails = useCallback(() => {
    if (!updateRef.current) return;
    setDetailsOpen(true);
    setNoticeVisible(false);
  }, []);

  const closeDetails = useCallback(() => {
    setDetailsOpen(false);
  }, []);

  const skipCurrentUpdate = useCallback(async () => {
    const current = updateRef.current;
    if (!current || busyRef.current) return;
    setSkippedVersionState(current.version);
    setNoticeVisible(false);
    setDetailsOpen(false);
    try {
      await saveSkippedUpdateVersion(current.version);
    } catch (reason) {
      console.warn("Failed to persist skipped update version", reason);
    }
  }, []);

  const clearSkippedVersion = useCallback(async () => {
    if (busyRef.current) return;
    setSkippedVersionState(null);
    try {
      await saveSkippedUpdateVersion(null);
    } catch (reason) {
      console.warn("Failed to clear skipped update version", reason);
    }
  }, []);

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
      setSkippedVersionState(null);
      void saveSkippedUpdateVersion(null).catch(() => undefined);
      liveNotesAbortRef.current?.abort();
      setUpdate(null);
      setLiveNotes(null);
      setLiveNotesLoading(false);
      setNoticeVisible(false);
      setDetailsOpen(false);
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

  const hasUnreadUpdate = Boolean(update && update.version !== skippedVersion);

  return {
    buildInfo,
    automaticChecks,
    receiveNightlyUpdates,
    skippedVersion,
    lastCheckedAt,
    preferenceReady,
    preferenceSaving,
    phase,
    update,
    liveNotes,
    liveNotesLoading,
    errorCode,
    downloadedBytes,
    totalBytes,
    downloadSpeedBytesPerSec,
    noticeVisible,
    detailsOpen,
    hasUnreadUpdate,
    check,
    install,
    retry,
    openDetails,
    closeDetails,
    skipCurrentUpdate,
    clearSkippedVersion,
    dismissNotice: () => setNoticeVisible(false),
    setAutomaticChecks,
    setReceiveNightlyUpdates,
  };
}
