import { useEffect, useRef, useState } from "react";

import type { AsrCapabilities } from "../providers/types";

export function useCudaRuntimeWarning(
  capabilities: AsrCapabilities | null,
  active: boolean,
) {
  const [open, setOpen] = useState(false);
  const shownRef = useRef(false);

  useEffect(() => {
    if (!active) return;
    const runtimeMissing = Boolean(
      capabilities
      && capabilities.cuda.device_count > 0
      && !capabilities.cuda.available,
    );
    if (runtimeMissing && !shownRef.current) {
      shownRef.current = true;
      setOpen(true);
    } else if (!runtimeMissing) {
      shownRef.current = false;
      setOpen(false);
    }
  }, [active, capabilities]);

  return {
    open,
    close: () => setOpen(false),
  };
}
