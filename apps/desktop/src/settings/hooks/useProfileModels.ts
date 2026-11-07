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

  const refresh = useCallback(async (force = true) => {
    const currentRequest = ++request.current;
    if (!profile || !enabled) {
      setLoading(false);
      return;
    }
    setLoading(true);
    setError("");
    try {
      const value = await loadModels(profile.id, { refresh: force });
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
    if (!enabled || !scope) {
      setLoading(false);
      return;
    }
    if (catalog.scope !== scope) void refresh(false);
    return () => { request.current += 1; };
  }, [catalog.scope, enabled, refresh, scope]);

  const value = catalog.scope === scope ? catalog.value : EMPTY_CATALOG;
  const scopePending = enabled && Boolean(profile) && catalog.scope !== scope;
  return {
    models: value.models,
    modelDetails: value.model_details ?? {},
    loading: scopePending || loading,
    error: scopePending || !enabled ? "" : error,
    refresh,
    load,
  };
}
