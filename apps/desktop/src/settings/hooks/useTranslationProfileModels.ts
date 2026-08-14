import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { coreApi } from "../../api";
import { localizedError } from "../../app-utils";
import type { ApiProfileView } from "../../types";

export function useTranslationProfileModels(
  profile: ApiProfileView | undefined,
  enabled: boolean,
) {
  const { t } = useTranslation();
  const [models, setModels] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const request = useRef(0);

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
      const response = await coreApi.apiProfileModels(profile.id);
      if (currentRequest === request.current) setModels(response.models);
    } catch (reason) {
      if (currentRequest === request.current) {
        setModels([]);
        setError(localizedError(reason, t, "errors.apiProfiles.models"));
      }
    } finally {
      if (currentRequest === request.current) setLoading(false);
    }
  }, [enabled, profile?.id, t]);

  useEffect(() => {
    void refresh();
    return () => { request.current += 1; };
  }, [refresh]);

  return { models, loading, error, refresh };
}
