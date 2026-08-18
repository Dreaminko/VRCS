import { useEffect, useState } from "react";

import {
  applyInterfaceScale,
  interfaceScaleShortcutStep,
  normalizeInterfaceScale,
  readInterfaceScale,
  syncInterfaceViewportProperties,
  writeInterfaceScale,
} from "./interface-scale";

type ReportError = (
  reason: unknown,
  fallbackKey: string,
  source?: string,
) => void;

export function useInterfaceScale(reportError: ReportError) {
  const [interfaceScale, setInterfaceScale] = useState(readInterfaceScale);

  useEffect(() => {
    writeInterfaceScale(interfaceScale);
    void applyInterfaceScale(interfaceScale).catch((reason) => {
      reportError(reason, "errors.window.interfaceScale", "window");
    });
  }, [interfaceScale, reportError]);

  useEffect(() => {
    window.addEventListener("resize", syncInterfaceViewportProperties);
    return () => window.removeEventListener("resize", syncInterfaceViewportProperties);
  }, []);

  useEffect(() => {
    const handleInterfaceScaleShortcut = (event: KeyboardEvent) => {
      const step = interfaceScaleShortcutStep(event);
      if (step === 0) return;
      event.preventDefault();
      setInterfaceScale((current) => normalizeInterfaceScale(current + step));
    };
    window.addEventListener("keydown", handleInterfaceScaleShortcut);
    return () => window.removeEventListener("keydown", handleInterfaceScaleShortcut);
  }, []);

  return { interfaceScale, setInterfaceScale };
}
