import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import { NATIVE_APP } from "../app/app-environment";
import { runtimeApi } from "../core-client/runtime-api";
import {
  applyInterfaceScale,
  DEFAULT_INTERFACE_SCALE,
  readInterfaceScale,
} from "../app/interface-scale";
import type { CompactFontSize } from "./compact-layout";
import {
  clampCompactWindowHeight,
  compactWindowConstraints,
  COMPACT_WINDOW_SIZE,
  compactWindowSize,
  readCompactAlwaysOnTop,
  readCompactDisplayMode,
  writeCompactAlwaysOnTop,
  writeCompactDisplayMode,
  type CompactDisplayMode,
  type CompactPanelState,
} from "./compact-mode";

export function useCompactWindow({
  alwaysOnTopAvailable,
  clearErrorFrom,
  reportError,
}: {
  alwaysOnTopAvailable?: boolean;
  clearErrorFrom: (source: string) => void;
  reportError: (reason: unknown, fallbackKey: string, source?: string) => void;
}) {
  const [compact, setCompact] = useState(false);
  const [displayMode, setDisplayMode] = useState<CompactDisplayMode>(() => readCompactDisplayMode());
  const toggleDisplayMode = useCallback(() => {
    setDisplayMode((previous) => {
      const next: CompactDisplayMode = previous === "fixed" ? "scroll" : "fixed";
      writeCompactDisplayMode(next);
      return next;
    });
  }, []);
  const [fontSize, setFontSize] = useState<CompactFontSize>("medium");
  const [alwaysOnTop, setAlwaysOnTop] = useState<boolean>(() => readCompactAlwaysOnTop());
  const alwaysOnTopEnabledRef = useRef(alwaysOnTop);
  alwaysOnTopEnabledRef.current = alwaysOnTop;
  const [transparentBackground, setTransparentBackground] = useState(false);
  const [backgroundOpacity, setBackgroundOpacity] = useState(100);
  const toggleCompactBackground = useCallback(() => {
    setTransparentBackground((previous) => !previous);
  }, []);
  const [height, setHeight] = useState<number>(COMPACT_WINDOW_SIZE.height);
  const alwaysOnTopRef = useRef(alwaysOnTopAvailable);
  alwaysOnTopRef.current = alwaysOnTopAvailable;
  const topmostTaskRef = useRef<Promise<void> | null>(null);
  const topmostPendingRef = useRef(false);
  const compactModeRef = useRef(false);
  const preCompactWindowRef = useRef<{
    width: number;
    height: number;
    x?: number;
    y?: number;
    maximized: boolean;
  } | null>(null);
  // null means a native adjustment was interrupted or only partially applied.
  const panelStateRef = useRef<CompactPanelState | null>(false);
  const requestedPanelStateRef = useRef<CompactPanelState>(false);
  const panelTaskRef = useRef<{ promise: Promise<void>; running: boolean } | null>(null);
  const userHeightRef = useRef<number>(COMPACT_WINDOW_SIZE.height);
  const autoCaptionHeightRef = useRef<number | null>(null);
  const heightRef = useRef<number>(COMPACT_WINDOW_SIZE.height);
  const nativeHeightRef = useRef<number>(COMPACT_WINDOW_SIZE.height);

  const rememberUserResize = useCallback((nativeHeight: number) => {
    // Apply only the user's delta, not temporary composer space or capped height.
    const nextHeight = clampCompactWindowHeight(heightRef.current + nativeHeight - nativeHeightRef.current);
    userHeightRef.current = nextHeight;
    autoCaptionHeightRef.current = null;
    heightRef.current = nextHeight;
    nativeHeightRef.current = nativeHeight;
    setHeight(nextHeight);
  }, []);

  useEffect(() => {
    if (!NATIVE_APP || !compact) return;

    let disposed = false;
    let heightReadId = 0;
    const syncHeight = async () => {
      const task = panelTaskRef.current;
      const panelState = panelStateRef.current;
      if (!compactModeRef.current || panelState === true || panelState === null || task?.running) return;
      const readId = ++heightReadId;
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const appWindow = getCurrentWindow();
        const [innerSize, scaleFactor] = await Promise.all([
          appWindow.innerSize(),
          appWindow.scaleFactor(),
        ]);
        // Read native geometry for delayed resize events, and discard reads
        // that overlap a panel adjustment, even if that task already finished.
        if (disposed || readId !== heightReadId || !compactModeRef.current || panelStateRef.current !== panelState
          || task !== panelTaskRef.current || task?.running) return;
        rememberUserResize(innerSize.toLogical(scaleFactor).height);
      } catch (reason) {
        if (!disposed && compactModeRef.current) {
          reportError(reason, "errors.window.compactResize", "window");
        }
      }
    };
    let resizeFrame: number | null = null;
    const onResize = () => {
      if (resizeFrame !== null) return;
      resizeFrame = window.requestAnimationFrame(() => {
        resizeFrame = null;
        void syncHeight();
      });
    };
    void syncHeight();
    window.addEventListener("resize", onResize);
    return () => {
      disposed = true;
      if (resizeFrame !== null) window.cancelAnimationFrame(resizeFrame);
      window.removeEventListener("resize", onResize);
    };
  }, [compact, rememberUserResize, reportError]);

  const resizeCompactWindow = useCallback((panelState: CompactPanelState): Promise<void> => {
    if (!compactModeRef.current) return Promise.resolve();
    requestedPanelStateRef.current = panelState;
    const currentTask = panelTaskRef.current;
    if (currentTask?.running) return currentTask.promise;
    const atTarget = (target: CompactPanelState) => panelStateRef.current === target
      && nativeHeightRef.current === compactWindowSize(target, 0, heightRef.current).height;
    if (atTarget(panelState)) return Promise.resolve();
    if (!NATIVE_APP) {
      panelStateRef.current = panelState;
      nativeHeightRef.current = compactWindowSize(panelState, 0, heightRef.current).height;
      return Promise.resolve();
    }

    // Mark the task before the first asynchronous read so resize events cannot
    // save the panel's height as the subtitle's restore height.
    const task = { promise: Promise.resolve(), running: true };
    panelTaskRef.current = task;
    task.promise = (async () => {
      let failure: { reason: unknown } | undefined;
      try {
        while (compactModeRef.current) {
          const target = requestedPanelStateRef.current;
          if (atTarget(target)) break;
          try {
            const syncUserHeight = panelStateRef.current !== true && panelStateRef.current !== null;
            const previousNativeHeight = nativeHeightRef.current;
            panelStateRef.current = null;
            const result = await invoke<{ width: number; height: number; baseHeight: number }>(
              "resize_compact_window",
              {
                args: {
                  baseHeight: heightRef.current,
                  chatboxHeight: typeof target === "number" ? target : 0,
                  overlayOpen: target === true,
                  syncUserHeight,
                  previousNativeHeight,
                },
              },
            );
            if (!compactModeRef.current) break;
            if (syncUserHeight && result.baseHeight !== heightRef.current) {
              userHeightRef.current = result.baseHeight;
              autoCaptionHeightRef.current = null;
              heightRef.current = result.baseHeight;
              setHeight(result.baseHeight);
            }
            panelStateRef.current = target;
            nativeHeightRef.current = result.height;
          } catch (reason) {
            failure = { reason };
            // Process a newer target after failure, but never retry indefinitely.
            if (requestedPanelStateRef.current === target) break;
          }
        }
        if (failure) throw failure.reason;
      } finally {
        task.running = false;
      }
    })();
    return task.promise;
  }, []);

  const ensureCaptionHeight = useCallback(async (minimum: number) => {
    if (!compactModeRef.current || requestedPanelStateRef.current === true) return;
    let target: number;
    if (minimum <= 0) {
      if (autoCaptionHeightRef.current === null) return;
      autoCaptionHeightRef.current = null;
      target = userHeightRef.current;
    } else {
      const clamped = clampCompactWindowHeight(minimum);
      autoCaptionHeightRef.current = clamped;
      target = Math.max(userHeightRef.current, clamped);
    }
    if (target === heightRef.current) return;
    heightRef.current = target;
    setHeight(target);
    try {
      await resizeCompactWindow(requestedPanelStateRef.current);
    } catch (reason) {
      reportError(reason, "errors.window.compactResize", "window");
    }
  }, [reportError, resizeCompactWindow]);

  const resolveCanTopmost = useCallback(async () => (
    alwaysOnTopRef.current
    ?? (await runtimeApi.platformCapabilities())?.always_on_top.available
    ?? false
  ), []);

  const setCompactMode = useCallback(async (
    next: boolean,
    onExitCompact: () => void = () => undefined,
  ): Promise<boolean> => {
    const previousMode = compactModeRef.current;
    compactModeRef.current = next;
    try {
      if (!next) {
        // Stop new panel work immediately and let any issued native call finish
        // before restoring the main window. Its caller reports panel failures.
        await panelTaskRef.current?.promise.catch(() => undefined);
      }
      await topmostTaskRef.current?.catch(() => undefined);
      if (!NATIVE_APP) {
        panelStateRef.current = false;
        requestedPanelStateRef.current = false;
        nativeHeightRef.current = heightRef.current;
        if (!next) onExitCompact();
        await applyInterfaceScale(next ? DEFAULT_INTERFACE_SCALE : readInterfaceScale());
        setCompact(next);
        clearErrorFrom("window");
        return true;
      }

      const { getCurrentWindow, LogicalPosition, LogicalSize } = await import(
        "@tauri-apps/api/window"
      );
      const appWindow = getCurrentWindow();
      const canTopmost = await resolveCanTopmost();
      if (next) {
        try {
          const [maximized, innerSize, outerPosition, scaleFactor] = await Promise.all([
            appWindow.isMaximized().catch(() => false),
            appWindow.innerSize(),
            appWindow.outerPosition().catch(() => null),
            appWindow.scaleFactor(),
          ]);
          if (maximized) {
            preCompactWindowRef.current = { width: 1180, height: 760, maximized: true };
            await appWindow.unmaximize().catch(() => undefined);
          } else {
            const logicalSize = innerSize.toLogical(scaleFactor);
            const logicalPos = outerPosition?.toLogical(scaleFactor);
            preCompactWindowRef.current = {
              width: Math.max(860, Math.round(logicalSize.width)),
              height: Math.max(620, Math.round(logicalSize.height)),
              x: logicalPos ? Math.round(logicalPos.x) : undefined,
              y: logicalPos ? Math.round(logicalPos.y) : undefined,
              maximized: false,
            };
          }
        } catch {
          preCompactWindowRef.current = null;
        }
        if (canTopmost) {
          await invoke("set_compact_window_topmost", { enabled: alwaysOnTopEnabledRef.current });
        }
        panelStateRef.current = false;
        requestedPanelStateRef.current = false;
        const compactSize = compactWindowSize(
          false,
          COMPACT_WINDOW_SIZE.width,
          heightRef.current,
        );
        await appWindow.setSizeConstraints(compactWindowConstraints(false));
        await appWindow.setSize(new LogicalSize(compactSize.width, compactSize.height));
        nativeHeightRef.current = compactSize.height;
        await appWindow.setResizable(true);
        setHeight(compactSize.height);
      } else {
        const saved = preCompactWindowRef.current;
        preCompactWindowRef.current = null;
        autoCaptionHeightRef.current = null;
        heightRef.current = userHeightRef.current;
        if (canTopmost) await invoke("set_compact_window_topmost", { enabled: false });
        await appWindow.setResizable(true);
        await appWindow.setSizeConstraints({ minWidth: 860, minHeight: 620 });
        await appWindow.setSize(new LogicalSize(saved?.width ?? 1180, saved?.height ?? 760));
        if (saved?.x !== undefined && saved?.y !== undefined) {
          await appWindow.setPosition(new LogicalPosition(saved.x, saved.y)).catch(() => undefined);
        }
        if (saved?.maximized) {
          await appWindow.maximize().catch(() => undefined);
        }
        panelStateRef.current = false;
        onExitCompact();
      }

      await appWindow.setShadow(!next);
      await applyInterfaceScale(next ? DEFAULT_INTERFACE_SCALE : readInterfaceScale());
      setCompact(next);
      clearErrorFrom("window");
      return true;
    } catch (reason) {
      compactModeRef.current = previousMode;
      void applyInterfaceScale(previousMode ? DEFAULT_INTERFACE_SCALE : readInterfaceScale());
      reportError(
        typeof reason === "string" ? new Error(reason) : reason,
        "errors.window.compactToggle",
        "window",
      );
      return false;
    }
  }, [clearErrorFrom, reportError, resolveCanTopmost]);

  const toggleAlwaysOnTop = useCallback(async (): Promise<boolean> => {
    if (topmostPendingRef.current) return false;
    const next = !alwaysOnTopEnabledRef.current;
    if (!NATIVE_APP) {
      alwaysOnTopEnabledRef.current = next;
      setAlwaysOnTop(next);
      writeCompactAlwaysOnTop(next);
      clearErrorFrom("window");
      return true;
    }

    topmostPendingRef.current = true;
    let applied = false;
    let task: Promise<void> | null = null;
    task = (async () => {
      try {
        const canTopmost = await resolveCanTopmost();
        if (!canTopmost || !compactModeRef.current) return;
        await invoke("set_compact_window_topmost", { enabled: next });
        alwaysOnTopEnabledRef.current = next;
        setAlwaysOnTop(next);
        writeCompactAlwaysOnTop(next);
        clearErrorFrom("window");
        applied = true;
      } catch (reason) {
        reportError(
          typeof reason === "string" ? new Error(reason) : reason,
          "errors.window.alwaysOnTop",
          "window",
        );
      } finally {
        topmostPendingRef.current = false;
        if (topmostTaskRef.current === task) topmostTaskRef.current = null;
      }
    })();
    topmostTaskRef.current = task;
    await task;
    return applied;
  }, [clearErrorFrom, reportError, resolveCanTopmost]);

  const enterCompact = useCallback(
    () => setCompactMode(true),
    [setCompactMode],
  );
  const exitCompact = useCallback(
    (onExitCompact?: () => void) => setCompactMode(false, onExitCompact),
    [setCompactMode],
  );

  const startCompactResize = useCallback(async () => {
    if (!NATIVE_APP) return;
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().startResizeDragging("SouthEast");
    } catch (reason) {
      reportError(reason, "errors.window.compactResize", "window");
    }
  }, [reportError]);

  const closeWindow = useCallback(async () => {
    if (!NATIVE_APP) {
      setCompact(false);
      return;
    }
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().close();
    } catch (reason) {
      console.warn("Could not close compact window", reason);
    }
  }, []);

  return useMemo(() => ({
    compact,
    displayMode,
    toggleDisplayMode,
    fontSize,
    setFontSize,
    alwaysOnTop,
    alwaysOnTopAvailable,
    toggleAlwaysOnTop,
    transparentBackground,
    backgroundOpacity,
    setBackgroundOpacity,
    toggleCompactBackground,
    height,
    resizeCompactWindow,
    ensureCaptionHeight,
    enterCompact,
    exitCompact,
    closeWindow,
    startCompactResize,
  }), [
    alwaysOnTop,
    alwaysOnTopAvailable,
    backgroundOpacity,
    closeWindow,
    compact,
    displayMode,
    ensureCaptionHeight,
    enterCompact,
    exitCompact,
    fontSize,
    height,
    resizeCompactWindow,
    startCompactResize,
    toggleAlwaysOnTop,
    toggleCompactBackground,
    toggleDisplayMode,
    transparentBackground,
  ]);
}
