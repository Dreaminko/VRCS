import { useCallback, useEffect, useRef, useState } from "react";

import {
  CONTRIBUTOR_REFRESH_COOLDOWN, CONTRIBUTOR_REFRESH_INTERVAL,
  ContributorRateLimitError, fetchContributors, loadContributorCache, saveContributorCache,
} from "./contributors";

let nextAttemptAt = 0;

export function useContributors() {
  const [cache, setCache] = useState(loadContributorCache);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<"failed" | "rateLimited" | null>(null);
  const request = useRef<AbortController | null>(null);
  const cachedResult = useRef(cache);

  const refresh = useCallback(async () => {
    if (request.current || document.hidden || Date.now() < nextAttemptAt) return;
    if (cachedResult.current
      && Date.now() - cachedResult.current.checkedAt < CONTRIBUTOR_REFRESH_INTERVAL) return;
    const controller = new AbortController();
    request.current = controller;
    nextAttemptAt = Date.now() + CONTRIBUTOR_REFRESH_COOLDOWN;
    setLoading(true);
    setError(null);
    try {
      const contributors = await fetchContributors(controller.signal);
      if (controller.signal.aborted) return;
      const result = { contributors, checkedAt: Date.now() };
      saveContributorCache(result);
      cachedResult.current = result;
      setCache(result);
    } catch (failure) {
      if (controller.signal.aborted) return;
      if (failure instanceof ContributorRateLimitError) {
        nextAttemptAt = failure.retryAt;
        setError("rateLimited");
      } else {
        setError("failed");
      }
    } finally {
      if (request.current === controller) {
        request.current = null;
        setLoading(false);
      }
    }
  }, []);

  useEffect(() => {
    const check = () => { void refresh(); };
    const initialCheck = window.setTimeout(check, 0);
    const timer = window.setInterval(check, CONTRIBUTOR_REFRESH_INTERVAL);
    document.addEventListener("visibilitychange", check);
    return () => {
      window.clearInterval(timer);
      window.clearTimeout(initialCheck);
      document.removeEventListener("visibilitychange", check);
      if (request.current) {
        request.current.abort();
        request.current = null;
        nextAttemptAt = 0;
      }
    };
  }, [refresh]);

  return { cache, loading, error };
}
