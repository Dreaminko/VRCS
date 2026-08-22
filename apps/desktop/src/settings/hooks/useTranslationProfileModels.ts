import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { providersApi } from "../../providers/api";
import { localizedError } from "../../app/app-utils";
import type { ApiProfileView } from "../../providers/types";

export function useTranslationProfileModels(
  profile: ApiProfileView | undefined,
  enabled: boolean,
) {
  const { t } = useTranslation();
  const [models, setModels] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const request = useRef(0);

  const load = useCallback(async (profileId: string) => {
    const response = await providersApi.apiProfileModels(profileId);
    return response.models;
  }, []);

  const refresh = useCallback(async () => {
    if (!profile || !enabled) {
      setModels([]);
      setError("");
      setLoading(false);
      return;
    }
    const currentRequest = ++request.current;
    setLoading(true);
    setError("");
    try {
      const nextModels = await load(profile.id);
      if (currentRequest === request.current) setModels(nextModels);
    } catch (reason) {
      if (currentRequest === request.current) {
        setModels([]);
        setError(localizedError(reason, t, "errors.apiProfiles.models"));
      }
    } finally {
      if (currentRequest === request.current) setLoading(false);
    }
  }, [enabled, load, profile?.id, t]);

  useEffect(() => {
    void refresh();
    return () => { request.current += 1; };
  }, [refresh]);

  return { models, loading, error, refresh, load };
}
