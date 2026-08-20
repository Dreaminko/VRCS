import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import { NATIVE_APP } from "../app/app-environment";
import {
  compactWindowConstraints,
  COMPACT_WINDOW_SIZE,
  compactWindowSize,
  type CompactPanelState,
} from "../compact-mode";

export function useCompactWindow({
  clearError,
  reportError,
}: {
  clearError: () => void;
  reportError: (reason: unknown, fallbackKey: string) => void;
}) {
  const [compact, setCompact] = useState(false);

  const resizeCompactWindow = useCallback(async (panelState: CompactPanelState) => {
    if (!NATIVE_APP) return;
    const { getCurrentWindow, LogicalSize } = await import(
      "@tauri-apps/api/window"
    );
    const appWindow = getCurrentWindow();
    const [innerSize, scaleFactor] = await Promise.all([
      appWindow.innerSize(),
      appWindow.scaleFactor(),
    ]);
    const currentWidth = innerSize.toLogical(scaleFactor).width;
    const size = compactWindowSize(panelState, currentWidth);
    await appWindow.setSizeConstraints(compactWindowConstraints(panelState));
    await appWindow.setSize(
      new LogicalSize(size.width, size.height),
    );
  }, []);

  const collapseCompactOverlay = useCallback(() => {
    void resizeCompactWindow(false).catch((reason) => {
      reportError(reason, "errors.window.compactCollapse");
    });
  }, [reportError, resizeCompactWindow]);

  const toggleCompact = useCallback(async (onExitCompact: () => void) => {
    const next = !compact;
    try {
      if (!NATIVE_APP) {
        if (!next) onExitCompact();
        setCompact(next);
        return;
      }

      const { getCurrentWindow, LogicalSize } = await import(
        "@tauri-apps/api/window"
      );
      const appWindow = getCurrentWindow();
      if (next) {
        const compactSize = new LogicalSize(
          COMPACT_WINDOW_SIZE.width,
          COMPACT_WINDOW_SIZE.height,
        );
        await appWindow.setSizeConstraints(compactWindowConstraints(false));
        await appWindow.setSize(compactSize);
        await appWindow.setResizable(true);
        await invoke("set_compact_window_topmost", { enabled: true });
      } else {
        onExitCompact();
        await invoke("set_compact_window_topmost", { enabled: false });
        await appWindow.setResizable(true);
        await appWindow.setSizeConstraints({ minWidth: 860, minHeight: 620 });
        await appWindow.setSize(new LogicalSize(1180, 760));
      }

      setCompact(next);
      clearError();
    } catch (reason) {
      reportError(
        typeof reason === "string" ? new Error(reason) : reason,
        "errors.window.compactToggle",
      );
    }
  }, [clearError, compact, reportError]);

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
    resizeCompactWindow,
    collapseCompactOverlay,
    toggleCompact,
    closeWindow,
  };
}
