import { useEffect, useState } from "react";

import {
  applyInterfaceScale,
  interfaceScaleShortcutStep,
  normalizeInterfaceScale,
  readInterfaceScale,
  syncInterfaceViewportProperties,
  writeInterfaceScale,
} from "./interface-scale";

export function useInterfaceScale() {
  const [interfaceScale, setInterfaceScale] = useState(readInterfaceScale);

  useEffect(() => {
    writeInterfaceScale(interfaceScale);
    applyInterfaceScale(interfaceScale);
  }, [interfaceScale]);

  useEffect(() => {
    let resizeFrame: number | null = null;
    const onResize = () => {
      if (resizeFrame !== null) return;
      resizeFrame = window.requestAnimationFrame(() => {
        resizeFrame = null;
        syncInterfaceViewportProperties();
      });
    };
    window.addEventListener("resize", onResize);
    return () => {
      if (resizeFrame !== null) window.cancelAnimationFrame(resizeFrame);
      window.removeEventListener("resize", onResize);
    };
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
