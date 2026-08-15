import { useCallback, useEffect, useRef, useState } from "react";

import { coreApi, coreWebSocketUrl } from "../api";
import {
  mergeSubtitleHistory,
  parseSubtitleStreamMessage,
  subtitleHistoryPage,
  SUBTITLE_HISTORY_REQUEST_LIMIT,
} from "../subtitle-stream";
import {
  clearAudioLevels,
  clearTranslationPartial,
  clearTranslationPartials,
  publishAudioLevel,
  publishTranslationPartial,
} from "../realtime-state";
import type {
  ConnectionState,
  LiveTranscription,
  Subtitle,
  SubtitleTranslation,
  VrchatMuteStatus,
} from "../types";

function withTranslation(
  subtitles: Subtitle[],
  subtitleId: number,
  translation: SubtitleTranslation,
): Subtitle[] {
  return subtitles.map((subtitle) => subtitle.id === subtitleId
    ? {
        ...subtitle,
        translations: [
          ...subtitle.translations.filter(
            (item) => item.target_language !== translation.target_language,
          ),
          translation,
        ],
        translation_partial: undefined,
      }
    : subtitle);
}

function withoutTranslationPartial(
  subtitles: Subtitle[],
  subtitleId: number,
): Subtitle[] {
  return subtitles.map((subtitle) => subtitle.id === subtitleId
    ? { ...subtitle, translation_partial: undefined }
    : subtitle);
}

export function useSubtitleStream({
  coreConfigured,
  reportError,
  clearErrorFrom,
}: {
  coreConfigured: boolean;
  reportError: (
    reason: unknown,
    fallbackKey: string,
    source?: string,
  ) => void;
  clearErrorFrom: (source: string) => void;
}) {
  const [connection, setConnection] = useState<ConnectionState>("connecting");
  const [subtitles, setSubtitles] = useState<Subtitle[]>([]);
  const [partials, setPartials] = useState<
    Partial<Record<LiveTranscription["source"], LiveTranscription>>
  >({});
  const [hasOlderSubtitles, setHasOlderSubtitles] = useState(false);
  const [loadingOlderSubtitles, setLoadingOlderSubtitles] = useState(false);
  const oldestHistoryIdRef = useRef<number | null>(null);
  const historyInitializedRef = useRef(false);
  const loadingOlderSubtitlesRef = useRef(false);
  const [translatingSubtitleIds, setTranslatingSubtitleIds] = useState<number[]>([]);
  const [vrchatMuteStatus, setVrchatMuteStatus] = useState<VrchatMuteStatus | null>(null);

  const updateHistoryCursor = useCallback((items: Subtitle[], hasOlder: boolean) => {
    oldestHistoryIdRef.current = items.at(-1)?.id ?? null;
    historyInitializedRef.current = true;
    setHasOlderSubtitles(hasOlder);
  }, []);

  const mergeSnapshot = useCallback((historyItems: Subtitle[]) => {
    const page = subtitleHistoryPage(historyItems);
    if (!historyInitializedRef.current) updateHistoryCursor(page.items, page.hasOlder);
    setSubtitles((current) => mergeSubtitleHistory(current, page.items));
  }, [updateHistoryCursor]);

  const replaceSnapshot = useCallback((historyItems: Subtitle[]) => {
    const page = subtitleHistoryPage(historyItems);
    updateHistoryCursor(page.items, page.hasOlder);
    setSubtitles(page.items);
  }, [updateHistoryCursor]);

  const clearPartials = useCallback(() => {
    setPartials({});
    clearAudioLevels();
    clearTranslationPartials();
  }, []);

  useEffect(() => {
    if (!coreConfigured) return;
    const refreshHistory = () => {
      void coreApi.subtitles({ limit: SUBTITLE_HISTORY_REQUEST_LIMIT })
        .then(replaceSnapshot)
        .catch(() => undefined);
    };
    window.addEventListener("vrcs:subtitle-history-refresh", refreshHistory);
    return () => window.removeEventListener("vrcs:subtitle-history-refresh", refreshHistory);
  }, [coreConfigured, replaceSnapshot]);

  useEffect(() => {
    if (!coreConfigured) {
      setConnection("connecting");
      historyInitializedRef.current = false;
      oldestHistoryIdRef.current = null;
      setHasOlderSubtitles(false);
      clearAudioLevels();
      clearTranslationPartials();
      setVrchatMuteStatus(null);
      return;
    }
    let socket: WebSocket | null = null;
    let retry: number | null = null;
    let closed = false;
    const connect = () => {
      setConnection("connecting");
      socket = new WebSocket(coreWebSocketUrl());
      socket.onopen = () => {
        setConnection("connected");
        void coreApi.subtitles({ limit: SUBTITLE_HISTORY_REQUEST_LIMIT })
          .then(mergeSnapshot)
          .catch(() => undefined);
      };
      socket.onmessage = (event) => {
        const message = parseSubtitleStreamMessage(event.data);
        if (message === null) return;
        switch (message.type) {
          case "subtitle":
            setSubtitles((current) => mergeSubtitleHistory([message.subtitle], current));
            setPartials((current) => ({
              ...current,
              [message.subtitle.source ?? "speaker"]: undefined,
            }));
            clearErrorFrom(`stream:${message.subtitle.source ?? "speaker"}`);
            break;
          case "partial":
            setPartials((current) => ({ ...current, [message.source]: message }));
            clearErrorFrom(`stream:${message.source}`);
            break;
          case "audio_level":
            publishAudioLevel(message);
            break;
          case "vrchat_mute_status":
            setVrchatMuteStatus(message.status);
            setPartials((current) => message.status.muted
              ? { ...current, microphone: undefined }
              : current);
            break;
          case "failed":
            reportError(
              { code: message.code ?? "asr.cloud_connect_failed" },
              "errors.core.connect",
              `stream:${message.source ?? "unknown"}`,
            );
            break;
          case "translation_started":
            clearTranslationPartial(message.subtitle_id);
            setSubtitles((current) => withoutTranslationPartial(current, message.subtitle_id));
            setTranslatingSubtitleIds((current) => current.includes(message.subtitle_id)
              ? current
              : [...current, message.subtitle_id]);
            break;
          case "translation_partial":
            publishTranslationPartial(message.subtitle_id, {
              text: message.text,
              target_language: message.target_language,
            });
            break;
          case "translation_completed":
            clearTranslationPartial(message.subtitle_id);
            setSubtitles((current) => withTranslation(
              current,
              message.subtitle_id,
              message.translation,
            ));
            setTranslatingSubtitleIds((current) => (
              current.filter((id) => id !== message.subtitle_id)
            ));
            clearErrorFrom(`translation:${message.subtitle_id}`);
            break;
          case "translation_failed":
            clearTranslationPartial(message.subtitle_id);
            setSubtitles((current) => withoutTranslationPartial(current, message.subtitle_id));
            setTranslatingSubtitleIds((current) => (
              current.filter((id) => id !== message.subtitle_id)
            ));
            reportError(
              { code: message.code ?? "translation.request_failed" },
              "errors.translation.failed",
              `translation:${message.subtitle_id}`,
            );
            break;
        }
      };
      socket.onclose = () => {
        setConnection("disconnected");
        clearAudioLevels();
        clearTranslationPartials();
        setVrchatMuteStatus(null);
        if (!closed) retry = window.setTimeout(connect, 1500);
      };
    };
    connect();
    return () => {
      closed = true;
      if (retry !== null) window.clearTimeout(retry);
      socket?.close();
    };
  }, [clearErrorFrom, coreConfigured, mergeSnapshot, reportError]);

  const loadOlderSubtitles = useCallback(async () => {
    const beforeId = oldestHistoryIdRef.current;
    if (!hasOlderSubtitles || beforeId === null || loadingOlderSubtitlesRef.current) return;
    loadingOlderSubtitlesRef.current = true;
    setLoadingOlderSubtitles(true);
    try {
      const historyItems = await coreApi.subtitles({
        limit: SUBTITLE_HISTORY_REQUEST_LIMIT,
        beforeId,
      });
      const page = subtitleHistoryPage(historyItems);
      if (page.items.length) {
        oldestHistoryIdRef.current = page.items.at(-1)?.id ?? beforeId;
        setSubtitles((current) => mergeSubtitleHistory(current, page.items));
      }
      setHasOlderSubtitles(page.hasOlder);
    } catch (reason) {
      reportError(reason, "errors.core.connect", "subtitle-history");
    } finally {
      loadingOlderSubtitlesRef.current = false;
      setLoadingOlderSubtitles(false);
    }
  }, [hasOlderSubtitles, reportError]);

  const translateSubtitle = useCallback(async (subtitleId: number) => {
    setTranslatingSubtitleIds((current) => current.includes(subtitleId)
      ? current
      : [...current, subtitleId]);
    try {
      const translation = await coreApi.translateSubtitle(subtitleId);
      clearTranslationPartial(subtitleId);
      setSubtitles((current) => withTranslation(current, subtitleId, translation));
      clearErrorFrom(`translation:${subtitleId}`);
    } catch (reason) {
      clearTranslationPartial(subtitleId);
      setSubtitles((current) => withoutTranslationPartial(current, subtitleId));
      reportError(
        reason,
        "errors.translation.failed",
        `translation:${subtitleId}`,
      );
    } finally {
      setTranslatingSubtitleIds((current) => current.filter((id) => id !== subtitleId));
    }
  }, [clearErrorFrom, reportError]);

  return {
    connection,
    subtitles,
    partials,
    hasOlderSubtitles,
    loadingOlderSubtitles,
    loadOlderSubtitles,
    vrchatMuteStatus,
    translatingSubtitleIds,
    mergeSnapshot,
    clearPartials,
    translateSubtitle,
  };
}
