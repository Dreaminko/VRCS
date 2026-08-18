import { useEffect, useState } from "react";

import { coreApi } from "../../api";
import type { ApiProfileView } from "../../types";

export function useApiProfileViews(refreshKey: unknown): ApiProfileView[] {
  const [profiles, setProfiles] = useState<ApiProfileView[]>([]);

  useEffect(() => {
    let cancelled = false;
    void coreApi.apiProfiles().then(
      (response) => {
        if (!cancelled) setProfiles(response.profiles);
      },
      () => {
        if (!cancelled) setProfiles([]);
      },
    );
    return () => { cancelled = true; };
  }, [refreshKey]);

  return profiles;
}
