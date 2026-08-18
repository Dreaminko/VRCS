import { useEffect, useRef, useState } from "react";

import type { VrchatMuteStatus } from "../types";

export type VrchatMuteToastState = {
  messageKey: string;
  muted: boolean;
};

export function useVrchatMuteToast({
  settingsReady,
  enabled,
  status,
}: {
  settingsReady: boolean;
  enabled: boolean;
  status: VrchatMuteStatus | null;
}): VrchatMuteToastState | null {
  const [toast, setToast] = useState<VrchatMuteToastState | null>(null);
  const previousMuteRef = useRef<boolean | null | undefined>(undefined);
  const muted = status?.muted ?? null;
  const syncEnabled = status?.enabled ?? false;

  useEffect(() => {
    if (!settingsReady) return;
    if (!enabled) {
      previousMuteRef.current = muted;
      setToast(null);
      return;
    }
    if (!syncEnabled || muted === null) {
      setToast(null);
      return;
    }
    const previous = previousMuteRef.current;
    previousMuteRef.current = muted;
    if (previous === muted || (previous === undefined && !muted)) return;
    setToast({
      muted,
      messageKey: muted
        ? "settings.osc.muteToastMuted"
        : "settings.osc.muteToastUnmuted",
    });
    const timer = window.setTimeout(() => setToast(null), 3_000);
    return () => window.clearTimeout(timer);
  }, [enabled, muted, settingsReady, syncEnabled]);

  return toast;
}
