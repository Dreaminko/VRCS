import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import { NATIVE_APP } from "../app/app-environment";
import type { CompactFontSize } from "./compact-layout";
import {
  clampCompactWindowHeight,
  compactWindowConstraints,
  COMPACT_WINDOW_SIZE,
  compactWindowSize,
  type CompactPanelState,
} from "./compact-mode";

export function useCompactWindow({
  clearErrorFrom,
  reportError,
}: {
  clearErrorFrom: (source: string) => void;
  reportError: (reason: unknown, fallbackKey: string, source?: string) => void;
}) {
  const [compact, setCompact] = useState(false);
  const [fontSize, setFontSize] = useState<CompactFontSize>("medium");
  const [transparentBackground, setTransparentBackground] = useState(false);
  const [backgroundOpacity, setBackgroundOpacity] = useState(100);
  const toggleCompactBackground = useCallback(() => {
    setTransparentBackground((previous) => !previous);
  }, []);
  const [height, setHeight] = useState<number>(COMPACT_WINDOW_SIZE.height);
  const compactModeRef = useRef(false);
  // null means a native adjustment was interrupted or only partially applied.
  const panelStateRef = useRef<CompactPanelState | null>(false);
  const requestedPanelStateRef = useRef<CompactPanelState>(false);
  const panelTaskRef = useRef<{ promise: Promise<void>; running: boolean } | null>(null);
  const heightRef = useRef<number>(COMPACT_WINDOW_SIZE.height);

  useEffect(() => {
    if (!NATIVE_APP || !compact) return;

    let disposed = false;
    let heightReadId = 0;
    const syncHeight = async () => {
      const task = panelTaskRef.current;
      if (!compactModeRef.current || panelStateRef.current !== false || task?.running) return;
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
        if (disposed || readId !== heightReadId || !compactModeRef.current || panelStateRef.current !== false
          || task !== panelTaskRef.current || task?.running) return;
        const nextHeight = clampCompactWindowHeight(innerSize.toLogical(scaleFactor).height);
        heightRef.current = nextHeight;
        setHeight(nextHeight);
      } catch (reason) {
        if (!disposed && compactModeRef.current) {
          reportError(reason, "errors.window.compactResize", "window");
        }
      }
    };
    const onResize = () => { void syncHeight(); };
    onResize();
    window.addEventListener("resize", onResize);
    return () => {
      disposed = true;
      window.removeEventListener("resize", onResize);
    };
  }, [compact, reportError]);

  const resizeCompactWindow = useCallback((panelState: CompactPanelState): Promise<void> => {
    if (!compactModeRef.current) return Promise.resolve();
    requestedPanelStateRef.current = panelState;
    const currentTask = panelTaskRef.current;
    if (currentTask?.running) return currentTask.promise;
    if (panelStateRef.current === panelState) return Promise.resolve();
    if (!NATIVE_APP) {
      panelStateRef.current = panelState;
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
          if (panelStateRef.current === target) break;
          try {
            const { getCurrentWindow, LogicalSize } = await import("@tauri-apps/api/window");
            if (!compactModeRef.current) break;
            const appWindow = getCurrentWindow();
            const [innerSize, scaleFactor] = await Promise.all([
              appWindow.innerSize(),
              appWindow.scaleFactor(),
            ]);
            if (!compactModeRef.current) break;
            const currentSize = innerSize.toLogical(scaleFactor);
            if (target && panelStateRef.current === false) {
              const currentHeight = clampCompactWindowHeight(currentSize.height);
              heightRef.current = currentHeight;
              setHeight(currentHeight);
            }
            const size = compactWindowSize(target, currentSize.width, heightRef.current);
            panelStateRef.current = null;
            await appWindow.setSizeConstraints(compactWindowConstraints(target));
            if (!compactModeRef.current) break;
            await appWindow.setSize(new LogicalSize(size.width, size.height));
            if (!compactModeRef.current) break;
            panelStateRef.current = target;
            if (!target) setHeight(size.height);
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
    if (!compactModeRef.current || panelStateRef.current !== false || panelTaskRef.current?.running) return;
    const target = clampCompactWindowHeight(minimum);
    if (target <= heightRef.current) return;
    heightRef.current = target;
    panelStateRef.current = null;
    try {
      await resizeCompactWindow(false);
      setHeight(target);
    } catch (reason) {
      reportError(reason, "errors.window.compactResize", "window");
    }
  }, [reportError, resizeCompactWindow]);

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
      if (!NATIVE_APP) {
        panelStateRef.current = false;
        if (!next) onExitCompact();
        setCompact(next);
        clearErrorFrom("window");
        return true;
      }

      const { getCurrentWindow, LogicalSize } = await import(
        "@tauri-apps/api/window"
      );
      const appWindow = getCurrentWindow();
      if (next) {
        panelStateRef.current = false;
        const compactSize = compactWindowSize(
          false,
          COMPACT_WINDOW_SIZE.width,
          heightRef.current,
        );
        await appWindow.setSizeConstraints(compactWindowConstraints(false));
        await appWindow.setSize(new LogicalSize(compactSize.width, compactSize.height));
        await appWindow.setResizable(true);
        await invoke("set_compact_window_topmost", { enabled: true });
        setHeight(compactSize.height);
      } else {
        await invoke("set_compact_window_topmost", { enabled: false });
        await appWindow.setResizable(true);
        await appWindow.setSizeConstraints({ minWidth: 860, minHeight: 620 });
        await appWindow.setSize(new LogicalSize(1180, 760));
        panelStateRef.current = false;
        onExitCompact();
      }

      await appWindow.setShadow(!next);
      setCompact(next);
      clearErrorFrom("window");
      return true;
    } catch (reason) {
      compactModeRef.current = previousMode;
      reportError(
        typeof reason === "string" ? new Error(reason) : reason,
        "errors.window.compactToggle",
        "window",
      );
      return false;
    }
  }, [clearErrorFrom, reportError]);

  const enterCompact = useCallback(
    () => setCompactMode(true),
    [setCompactMode],
  );
  const exitCompact = useCallback(
    (onExitCompact?: () => void) => setCompactMode(false, onExitCompact),
    [setCompactMode],
  );
  const toggleCompact = useCallback(
    (onExitCompact: () => void) => compact
      ? exitCompact(onExitCompact)
      : enterCompact(),
    [compact, enterCompact, exitCompact],
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
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().close();
    } catch {
      setCompact(false);
    }
  }, []);

  return {
    compact,
    fontSize,
    setFontSize,
    transparentBackground,
    backgroundOpacity,
    setBackgroundOpacity,
    toggleCompactBackground,
    height,
    resizeCompactWindow,
    ensureCaptionHeight,
    enterCompact,
    exitCompact,
    toggleCompact,
    closeWindow,
    startCompactResize,
  };
}
