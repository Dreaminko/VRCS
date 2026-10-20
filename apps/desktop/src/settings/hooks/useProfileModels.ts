import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { providersApi } from "../../providers/api";
import { localizedError } from "../../app/app-utils";
import type { ApiProfileView, ApiModelCatalog } from "../../providers/types";

const EMPTY_CATALOG: ApiModelCatalog = { models: [], model_details: {} };

export function useProfileModels(
  profile: ApiProfileView | undefined,
  enabled: boolean,
  loadModels = providersApi.apiProfileModels,
) {
  const { t } = useTranslation();
  const scope = enabled && profile ? JSON.stringify(profile) : "";
  const [catalog, setCatalog] = useState({ scope: "", value: EMPTY_CATALOG });
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const request = useRef(0);

  const load = useCallback(async (profileId: string) => {
    return loadModels(profileId);
  }, [loadModels]);

  const refresh = useCallback(async () => {
    const currentRequest = ++request.current;
    if (!profile || !enabled) {
      setCatalog({ scope, value: EMPTY_CATALOG });
      setError("");
      setLoading(false);
      return;
    }
    setLoading(true);
    setError("");
    try {
      const value = await loadModels(profile.id);
      if (currentRequest === request.current) {
        setCatalog({ scope, value });
      }
    } catch (reason) {
      if (currentRequest === request.current) {
        setCatalog({ scope, value: EMPTY_CATALOG });
        setError(localizedError(reason, t, "errors.apiProfiles.models"));
      }
    } finally {
      if (currentRequest === request.current) setLoading(false);
    }
  }, [enabled, loadModels, scope, t]);

  useEffect(() => {
    void refresh();
    return () => { request.current += 1; };
  }, [refresh]);

  const value = catalog.scope === scope ? catalog.value : EMPTY_CATALOG;
  return { models: value.models, modelDetails: value.model_details ?? {}, loading, error, refresh, load };
}
