import { useEffect, useState } from "react";

import { providersApi } from "../../providers/api";
import { API_PROFILES_CHANGED_EVENT } from "../../providers/api-profile-events";
import type {
  ApiProfileView,
  ProviderDefinition,
} from "../../providers/types";

export interface ApiProfileCatalog {
  profiles: ApiProfileView[];
  providerDefinitions: ProviderDefinition[];
}

const EMPTY_CATALOG: ApiProfileCatalog = { profiles: [], providerDefinitions: [] };

export function useApiProfileViews(refreshKey: unknown): ApiProfileCatalog {
  const [catalog, setCatalog] = useState<ApiProfileCatalog>(EMPTY_CATALOG);

  useEffect(() => {
    let cancelled = false;
    const load = () => {
      void Promise.all([providersApi.apiProfiles(), providersApi.providers()]).then(
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
    };
    load();
    window.addEventListener(API_PROFILES_CHANGED_EVENT, load);
    return () => {
      cancelled = true;
      window.removeEventListener(API_PROFILES_CHANGED_EVENT, load);
    };
  }, [refreshKey]);

  return catalog;
}
