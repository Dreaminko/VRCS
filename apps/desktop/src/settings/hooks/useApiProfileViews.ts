import { useEffect, useState } from "react";

import { coreApi } from "../../api";
import type { ApiProfileView, ProviderDefinition } from "../../types";

export interface ApiProfileCatalog {
  profiles: ApiProfileView[];
  providerDefinitions: ProviderDefinition[];
}

const EMPTY_CATALOG: ApiProfileCatalog = { profiles: [], providerDefinitions: [] };

export function useApiProfileViews(refreshKey: unknown): ApiProfileCatalog {
  const [catalog, setCatalog] = useState<ApiProfileCatalog>(EMPTY_CATALOG);

  useEffect(() => {
    let cancelled = false;
    void Promise.all([coreApi.apiProfiles(), coreApi.providers()]).then(
      ([profileResponse, providerResponse]) => {
        if (!cancelled) {
          setCatalog({
            profiles: profileResponse.profiles,
            providerDefinitions: providerResponse.providers,
          });
        }
      },
      () => {
        if (!cancelled) setCatalog(EMPTY_CATALOG);
      },
    );
    return () => { cancelled = true; };
  }, [refreshKey]);

  return catalog;
}
