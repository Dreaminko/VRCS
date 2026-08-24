import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import { NATIVE_APP } from "../app/app-environment";
import {
  clampCompactWindowHeight,
  compactWindowConstraints,
  COMPACT_WINDOW_SIZE,
  compactWindowSize,
  type CompactPanelState,
} from "../compact-mode";

export function useCompactWindow({
  clearErrorFrom,
  reportError,
}: {
  clearErrorFrom: (source: string) => void;
  reportError: (reason: unknown, fallbackKey: string, source?: string) => void;
}) {
  const [compact, setCompact] = useState(false);
  const [height, setHeight] = useState<number>(COMPACT_WINDOW_SIZE.height);
  const compactModeRef = useRef(false);
  const panelStateRef = useRef<CompactPanelState>(false);
  const heightRef = useRef<number>(COMPACT_WINDOW_SIZE.height);

  useEffect(() => {
    if (!NATIVE_APP || !compact) return;

    const syncHeight = () => {
      if (!compactModeRef.current || panelStateRef.current) return;
      const nextHeight = clampCompactWindowHeight(window.innerHeight);
      heightRef.current = nextHeight;
      setHeight(nextHeight);
    };
    syncHeight();
    window.addEventListener("resize", syncHeight);
    return () => window.removeEventListener("resize", syncHeight);
  }, [compact]);

  const resizeCompactWindow = useCallback(async (panelState: CompactPanelState) => {
    if (!NATIVE_APP) {
      panelStateRef.current = panelState;
      return;
    }
    const { getCurrentWindow, LogicalSize } = await import(
      "@tauri-apps/api/window"
    );
    const appWindow = getCurrentWindow();
    const [innerSize, scaleFactor] = await Promise.all([
      appWindow.innerSize(),
      appWindow.scaleFactor(),
    ]);
    const currentSize = innerSize.toLogical(scaleFactor);
    const previousPanelState = panelStateRef.current;
    if (panelState && !previousPanelState) {
      const currentHeight = clampCompactWindowHeight(currentSize.height);
      heightRef.current = currentHeight;
      setHeight(currentHeight);
    }

    panelStateRef.current = true;
    try {
      const size = compactWindowSize(
        panelState,
        currentSize.width,
        heightRef.current,
      );
      await appWindow.setSizeConstraints(compactWindowConstraints(panelState));
      await appWindow.setSize(new LogicalSize(size.width, size.height));
      panelStateRef.current = panelState;
      if (!panelState) setHeight(size.height);
    } catch (reason) {
      panelStateRef.current = previousPanelState;
      throw reason;
    }
  }, []);

  const collapseCompactOverlay = useCallback(() => {
    void resizeCompactWindow(false).catch((reason) => {
      reportError(reason, "errors.window.compactCollapse");
    });
  }, [reportError, resizeCompactWindow]);

  const setCompactMode = useCallback(async (
    next: boolean,
    onExitCompact: () => void = () => undefined,
  ): Promise<boolean> => {
    const previousMode = compactModeRef.current;
    compactModeRef.current = next;
    try {
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
    height,
    resizeCompactWindow,
    collapseCompactOverlay,
    enterCompact,
    exitCompact,
    toggleCompact,
    closeWindow,
  };
}
