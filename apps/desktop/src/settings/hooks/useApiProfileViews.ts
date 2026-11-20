import { useEffect, useRef } from "react";
import { refreshApiProfileCatalog, useApiProfileCatalog } from "../../providers/useApiProfileCatalog";

export function useApiProfileViews(refreshKey: unknown, enabled = true) {
  const catalog = useApiProfileCatalog(enabled);
  const previousKey = useRef(refreshKey);
  const wasEnabled = useRef(false);
  useEffect(() => {
    if (!enabled) {
      wasEnabled.current = false;
      previousKey.current = refreshKey;
      return;
    }
    if (!wasEnabled.current) {
      wasEnabled.current = true;
      previousKey.current = refreshKey;
      return;
    }
    if (Object.is(previousKey.current, refreshKey)) return;
    previousKey.current = refreshKey;
    void refreshApiProfileCatalog(true).catch(() => undefined);
  }, [enabled, refreshKey]);
  return catalog;
}
