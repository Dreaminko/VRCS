import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { NATIVE_APP } from "../app-environment";
import {
  compactWindowConstraints,
  COMPACT_WINDOW_SIZE,
  compactWindowSize,
} from "../compact-mode";

export function useCompactWindow({
  clearError,
  reportError,
}: {
  clearError: () => void;
  reportError: (reason: unknown, fallbackKey: string) => void;
}) {
  const { t } = useTranslation();
  const [compact, setCompact] = useState(false);

  const resizeCompactWindow = useCallback(async (lookupOpen: boolean) => {
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
    const size = compactWindowSize(lookupOpen, currentWidth);
    await appWindow.setSizeConstraints(compactWindowConstraints(lookupOpen));
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
        await appWindow.setAlwaysOnTop(true);
      } else {
        onExitCompact();
        await appWindow.setAlwaysOnTop(false);
        await appWindow.setResizable(true);
        await appWindow.setSizeConstraints({ minWidth: 860, minHeight: 620 });
        await appWindow.setSize(new LogicalSize(1180, 760));
      }

      if (await appWindow.isAlwaysOnTop() !== next) {
        throw new Error(t("errors.window.alwaysOnTop"));
      }
      setCompact(next);
      clearError();
    } catch (reason) {
      reportError(reason, "errors.window.compactToggle");
    }
  }, [clearError, compact, reportError, t]);

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
