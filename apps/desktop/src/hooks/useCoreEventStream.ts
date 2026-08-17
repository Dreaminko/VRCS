import { useCallback, useEffect, useRef, useState } from "react";

import { coreWebSocketUrl } from "../api";
import {
  clearAudioLevels,
  clearLivePartial,
  clearLivePartials,
  clearTranslationPartial,
  clearTranslationPartials,
  publishAudioLevel,
  publishLivePartial,
  publishTranslationPartial,
} from "../realtime-state";
import {
  parseSubtitleStreamMessage,
  type ConversationCatalogEvent,
} from "../subtitle-stream";
import type {
  ConnectionState,
  Subtitle,
  SubtitleTranslation,
  VrchatMuteStatus,
} from "../types";

type EventHandlers = {
  onConnected: () => void;
  onSubtitle: (subtitle: Subtitle) => void;
  onTranslationStarted: (subtitleId: number) => void;
  onTranslationCompleted: (
    subtitleId: number,
    translation: SubtitleTranslation,
  ) => void;
  onTranslationFailed: (subtitleId: number) => void;
};

export function useCoreEventStream({
  coreConfigured,
  handlers,
  reportError,
  clearErrorFrom,
}: {
  coreConfigured: boolean;
  handlers: EventHandlers;
  reportError: (reason: unknown, fallbackKey: string, source?: string) => void;
  clearErrorFrom: (source: string) => void;
}) {
  const [connection, setConnection] = useState<ConnectionState>("connecting");
  const [conversationCatalogEvent, setConversationCatalogEvent] = useState<ConversationCatalogEvent | null>(null);
  const [vrchatMuteStatus, setVrchatMuteStatus] = useState<VrchatMuteStatus | null>(null);
  const catalogSequenceRef = useRef(0);
  const handlersRef = useRef(handlers);
  const reportErrorRef = useRef(reportError);
  const clearErrorFromRef = useRef(clearErrorFrom);
  handlersRef.current = handlers;
  reportErrorRef.current = reportError;
  clearErrorFromRef.current = clearErrorFrom;

  const clearPartials = useCallback(() => {
    clearLivePartials();
    clearAudioLevels();
    clearTranslationPartials();
  }, []);

  useEffect(() => {
    if (!coreConfigured) {
      setConnection("connecting");
      clearPartials();
      setVrchatMuteStatus(null);
      setConversationCatalogEvent(null);
      return;
    }

    let socket: WebSocket | null = null;
    let retry: number | null = null;
    let closed = false;

    const connect = () => {
      setConnection("connecting");
      socket = new WebSocket(coreWebSocketUrl());
      socket.onopen = () => {
        if (closed) return;
        setConnection("connected");
        handlersRef.current.onConnected();
      };
      socket.onmessage = (event) => {
        const message = parseSubtitleStreamMessage(event.data);
        if (message === null) return;
        switch (message.type) {
          case "subtitle": {
            handlersRef.current.onSubtitle(message.subtitle);
            const source = message.subtitle.source ?? "speaker";
            if (source !== "chatbox") clearLivePartial(source);
            clearErrorFromRef.current(`stream:${source}`);
            break;
          }
          case "conversation_catalog":
            catalogSequenceRef.current += 1;
            setConversationCatalogEvent({
              sequence: catalogSequenceRef.current,
              catalog: message.catalog,
            });
            break;
          case "partial":
            publishLivePartial(message);
            clearErrorFromRef.current(`stream:${message.source}`);
            break;
          case "audio_level":
            publishAudioLevel(message);
            break;
          case "vrchat_mute_status":
            setVrchatMuteStatus(message.status);
            if (message.status.muted) clearLivePartial("microphone");
            break;
          case "failed":
            reportErrorRef.current(
              { code: message.code ?? "asr.cloud_connect_failed" },
              "errors.core.connect",
              `stream:${message.source ?? "unknown"}`,
            );
            break;
          case "translation_started":
            clearTranslationPartial(message.subtitle_id);
            handlersRef.current.onTranslationStarted(message.subtitle_id);
            break;
          case "translation_partial":
            publishTranslationPartial(message.subtitle_id, {
              text: message.text,
              target_language: message.target_language,
            });
            break;
          case "translation_completed":
            clearTranslationPartial(message.subtitle_id);
            handlersRef.current.onTranslationCompleted(
              message.subtitle_id,
              message.translation,
            );
            clearErrorFromRef.current(`translation:${message.subtitle_id}`);
            break;
          case "translation_failed":
            clearTranslationPartial(message.subtitle_id);
            handlersRef.current.onTranslationFailed(message.subtitle_id);
            reportErrorRef.current(
              { code: message.code ?? "translation.request_failed" },
              "errors.translation.failed",
              `translation:${message.subtitle_id}`,
            );
            break;
        }
      };
      socket.onclose = () => {
        if (closed) return;
        setConnection("disconnected");
        clearPartials();
        setVrchatMuteStatus(null);
        retry = window.setTimeout(connect, 1500);
      };
    };

    connect();
    return () => {
      closed = true;
      if (retry !== null) window.clearTimeout(retry);
      socket?.close();
    };
  }, [clearPartials, coreConfigured]);

  return {
    connection,
    conversationCatalogEvent,
    vrchatMuteStatus,
    clearPartials,
  };
}
