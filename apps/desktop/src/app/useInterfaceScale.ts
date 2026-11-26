import { useEffect, useState } from "react";

import {
  applyInterfaceScale,
  DEFAULT_INTERFACE_SCALE,
  interfaceScaleShortcutStep,
  normalizeInterfaceScale,
  readInterfaceScale,
  writeInterfaceScale,
} from "./interface-scale";

export function useInterfaceScale(compact = false) {
  const [interfaceScale, setInterfaceScale] = useState(readInterfaceScale);

  useEffect(() => {
    writeInterfaceScale(interfaceScale);
  }, [interfaceScale]);

  useEffect(() => {
    void applyInterfaceScale(compact ? DEFAULT_INTERFACE_SCALE : interfaceScale);
  }, [compact, interfaceScale]);

  useEffect(() => {
    if (compact) return;
    const handleInterfaceScaleShortcut = (event: KeyboardEvent) => {
      const step = interfaceScaleShortcutStep(event);
      if (step === 0) return;
      event.preventDefault();
      setInterfaceScale((current) => normalizeInterfaceScale(current + step));
    };
    window.addEventListener("keydown", handleInterfaceScaleShortcut);
    return () => window.removeEventListener("keydown", handleInterfaceScaleShortcut);
  }, [compact]);

  return { interfaceScale, setInterfaceScale };
}
