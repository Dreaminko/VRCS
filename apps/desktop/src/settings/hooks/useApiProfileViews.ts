import { useEffect, useRef } from "react";
import { refreshApiProfileCatalog, useApiProfileCatalog } from "../../providers/useApiProfileCatalog";

export function useApiProfileViews(refreshKey: unknown) {
  const catalog = useApiProfileCatalog();
  const previousKey = useRef(refreshKey);
  useEffect(() => {
    if (Object.is(previousKey.current, refreshKey)) return;
    previousKey.current = refreshKey;
    void refreshApiProfileCatalog(true).catch(() => undefined);
  }, [refreshKey]);
  return catalog;
}
