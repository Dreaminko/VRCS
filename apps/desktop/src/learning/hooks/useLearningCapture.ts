import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { learningApi } from "../api";
import { localizedError } from "../../app/app-utils";
import type { Lookup } from "../../app/app-types";
import {
  buildLookupLearningCapture,
  buildSubtitleLearningCapture,
  learningItemCaptureKeys,
  lookupLearningKey,
  subtitleLearningKey,
  subtitleSelectionLearningKey,
} from "../../learning";
import type {
  LearningItem,
  LearningItemCreateInput,
} from "../types";
import type { Subtitle } from "../../subtitles/types";

export function useLearningCapture(
  ready: boolean,
  onCollected: (item: LearningItem) => void,
) {
  const { t } = useTranslation();
  const [error, setError] = useState("");
  const [captureBusyKeys, setCaptureBusyKeys] = useState<Set<string>>(() => new Set());
  const [capturedKeys, setCapturedKeys] = useState<Set<string>>(() => new Set());
  const captureBusyRef = useRef(new Set<string>());
  const captureKeysRequestRef = useRef(0);

  const loadCaptureKeys = useCallback(async () => {
    const requestId = ++captureKeysRequestRef.current;
    setError("");
    try {
      const response = await learningApi.learningCaptureKeys();
      if (requestId !== captureKeysRequestRef.current) return;
      setCapturedKeys((current) => new Set([...response.keys, ...current]));
    } catch (reason) {
      if (requestId === captureKeysRequestRef.current) {
        setError(localizedError(reason, t, "errors.learning.load"));
      }
    }
  }, [t]);

  useEffect(() => {
    if (!ready) return;
    void loadCaptureKeys();
  }, [ready, loadCaptureKeys]);

  useEffect(() => () => {
    captureKeysRequestRef.current += 1;
  }, []);

  const collectCapture = useCallback(async (
    input: LearningItemCreateInput,
    key: string,
  ): Promise<LearningItem | null> => {
    if (captureBusyRef.current.has(key)) return null;
    captureBusyRef.current.add(key);
    setCaptureBusyKeys((current) => new Set(current).add(key));
    setError("");
    try {
      const item = await learningApi.createLearningItem(input);
      setCapturedKeys((current) => addCapturedItemKeys(new Set(current).add(key), [item]));
      onCollected(item);
      return item;
    } catch (reason) {
      setError(localizedError(reason, t, "errors.learning.collect"));
      return null;
    } finally {
      captureBusyRef.current.delete(key);
      setCaptureBusyKeys((current) => {
        const next = new Set(current);
        next.delete(key);
        return next;
      });
    }
  }, [onCollected, t]);

  const collectSubtitle = useCallback((subtitle: Subtitle) => {
    const input = buildSubtitleLearningCapture([subtitle]);
    return input ? collectCapture(input, subtitleLearningKey(subtitle)) : Promise.resolve(null);
  }, [collectCapture]);

  const collectSubtitles = useCallback((
    subtitles: Subtitle[],
    ids: Iterable<number>,
    options: { mergeFragments?: boolean } = {},
  ) => {
    const selectedIds = [...ids];
    const input = buildSubtitleLearningCapture(subtitles, selectedIds, options);
    return input
      ? collectCapture(input, subtitleSelectionLearningKey(selectedIds))
      : Promise.resolve(null);
  }, [collectCapture]);

  const collectLookup = useCallback((lookup: Lookup) => (
    collectCapture(buildLookupLearningCapture(lookup), lookupLearningKey(lookup))
  ), [collectCapture]);

  const refreshAfterDelete = useCallback(async (item: LearningItem | null) => {
    try {
      const response = await learningApi.learningCaptureKeys();
      setCapturedKeys(new Set(response.keys));
      setError("");
    } catch {
      if (item) {
        setCapturedKeys((current) => removeCapturedItemKeys(current, item));
      }
    }
  }, []);

  return {
    error,
    clearError: () => setError(""),
    isCollecting: (key: string) => captureBusyRef.current.has(key),
    isCaptured: (key: string) => capturedKeys.has(key),
    captureBusyKeys,
    collectSubtitle,
    collectSubtitles,
    collectLookup,
    refreshAfterDelete,
  };
}

function addCapturedItemKeys(current: Set<string>, items: LearningItem[]): Set<string> {
  const next = new Set(current);
  for (const item of items) {
    for (const key of learningItemCaptureKeys(item)) next.add(key);
  }
  return next;
}

function removeCapturedItemKeys(current: Set<string>, item: LearningItem): Set<string> {
  const next = new Set(current);
  for (const key of learningItemCaptureKeys(item)) next.delete(key);
  return next;
}
