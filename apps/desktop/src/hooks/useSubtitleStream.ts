import { useCallback, useEffect, useState } from "react";

import { coreApi, coreWebSocketUrl } from "../api";
import {
  mergeSubtitleHistory,
  parseSubtitleStreamMessage,
} from "../subtitle-stream";
import type {
  AudioLevel,
  ConnectionState,
  LiveTranscription,
  Subtitle,
  SubtitleTranslation,
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

function withTranslationPartial(
  subtitles: Subtitle[],
  subtitleId: number,
  text?: string,
  targetLanguage?: string,
): Subtitle[] {
  return subtitles.map((subtitle) => subtitle.id === subtitleId
    ? {
        ...subtitle,
        translation_partial: text && targetLanguage
          ? { text, target_language: targetLanguage }
          : undefined,
      }
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
  const [audioLevels, setAudioLevels] = useState<
    Partial<Record<AudioLevel["source"], AudioLevel>>
  >({});
  const [translatingSubtitleIds, setTranslatingSubtitleIds] = useState<number[]>([]);

  const mergeSnapshot = useCallback((historyItems: Subtitle[]) => {
    setSubtitles((current) => mergeSubtitleHistory(current, historyItems));
  }, []);

  const clearPartials = useCallback(() => {
    setPartials({});
    setAudioLevels({});
  }, []);

  useEffect(() => {
    if (!coreConfigured) {
      setConnection("connecting");
      setAudioLevels({});
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
        void coreApi.subtitles().then(mergeSnapshot).catch(() => undefined);
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
            setAudioLevels((current) => ({ ...current, [message.source]: message }));
            break;
          case "failed":
            reportError(
              { code: message.code ?? "asr.cloud_connect_failed" },
              "errors.core.connect",
              `stream:${message.source ?? "unknown"}`,
            );
            break;
          case "translation_started":
            setSubtitles((current) => withTranslationPartial(current, message.subtitle_id));
            setTranslatingSubtitleIds((current) => current.includes(message.subtitle_id)
              ? current
              : [...current, message.subtitle_id]);
            break;
          case "translation_partial":
            setSubtitles((current) => withTranslationPartial(
              current,
              message.subtitle_id,
              message.text,
              message.target_language,
            ));
            break;
          case "translation_completed":
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
            setSubtitles((current) => withTranslationPartial(current, message.subtitle_id));
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
        setAudioLevels({});
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

  const translateSubtitle = useCallback(async (subtitleId: number) => {
    setTranslatingSubtitleIds((current) => current.includes(subtitleId)
      ? current
      : [...current, subtitleId]);
    try {
      const translation = await coreApi.translateSubtitle(subtitleId);
      setSubtitles((current) => withTranslation(current, subtitleId, translation));
      clearErrorFrom(`translation:${subtitleId}`);
    } catch (reason) {
      setSubtitles((current) => withTranslationPartial(current, subtitleId));
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
    audioLevels,
    translatingSubtitleIds,
    mergeSnapshot,
    clearPartials,
    translateSubtitle,
  };
}
