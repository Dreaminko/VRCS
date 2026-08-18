import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { coreApi } from "../api";
import { localizedError } from "../app/app-utils";
import {
  applyChatboxPreferences,
  chatboxPreferencesFromDraft,
} from "./chatbox";
import {
  loadChatboxPreferences,
  saveChatboxPreferences,
} from "./chatbox-preferences";
import {
  clearSentDraft,
  createChatboxDraft,
  previewChatboxLocally,
} from "./chatbox";
import type {
  ChatboxComposeInput,
  ChatboxPreview,
  Settings,
} from "../types";

export function useChatboxWorkspace(settings: Settings | null) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState<ChatboxComposeInput>(() => createChatboxDraft());
  const [translationBasis, setTranslationBasis] = useState<{
    original: string;
    targetLanguage: string | null;
  } | null>(null);
  const [preview, setPreview] = useState<ChatboxPreview>(() => previewChatboxLocally(draft));
  const [busy, setBusy] = useState<"translate" | "send" | null>(null);
  const [feedback, setFeedback] = useState<{ tone: "success" | "error"; text: string } | null>(null);
  const previewRequest = useRef(0);
  const preferencesLoaded = useRef(false);
  const preferencesChanged = useRef(false);
  const translationFresh = Boolean(draft.translation?.trim())
    && translationBasis?.original === draft.original
    && translationBasis.targetLanguage === draft.target_language;

  useEffect(() => {
    if (!settings || preferencesLoaded.current) return;
    preferencesLoaded.current = true;
    void loadChatboxPreferences(settings.translation.target_language).then((preferences) => {
      if (preferencesChanged.current) return;
      setDraft((current) => applyChatboxPreferences(current, preferences));
    });
  }, [settings]);

  useEffect(() => {
    const local = previewChatboxLocally(draft);
    setPreview(local);
    const requestId = ++previewRequest.current;
    if (draft.send_mode !== "original" && !translationFresh) return;
    const timer = window.setTimeout(() => {
      void coreApi.previewChatbox(draft).then(
        (value) => {
          if (requestId === previewRequest.current) setPreview(value);
        },
        () => undefined,
      );
    }, 180);
    return () => window.clearTimeout(timer);
  }, [draft, translationFresh]);

  const changeDraft = useCallback((next: ChatboxComposeInput) => {
    if (next.translation !== draft.translation) {
      setTranslationBasis(next.translation?.trim()
        ? { original: next.original, targetLanguage: next.target_language }
        : null);
    }
    const currentPreferences = chatboxPreferencesFromDraft(draft);
    const nextPreferences = chatboxPreferencesFromDraft(next);
    if (JSON.stringify(currentPreferences) !== JSON.stringify(nextPreferences)) {
      preferencesChanged.current = true;
      void saveChatboxPreferences(nextPreferences);
    }
    setDraft(next);
    setFeedback(null);
  }, [draft.translation]);

  const show = useCallback(() => {
    setOpen(true);
    setFeedback(null);
  }, []);

  const close = useCallback(() => setOpen(false), []);

  const requestTranslation = useCallback(async (input: ChatboxComposeInput) => {
    const result = await coreApi.previewTranslation(
      input.original,
      input.source_language,
      input.target_language ?? undefined,
    );
    return {
      ...input,
      translation: result.text,
      source_language: result.source_language,
      target_language: result.target_language,
    };
  }, []);

  const rememberTranslation = useCallback((input: ChatboxComposeInput) => {
    setDraft(input);
    setTranslationBasis({
      original: input.original,
      targetLanguage: input.target_language,
    });
  }, []);

  const translate = useCallback(async () => {
    if (!draft.original.trim() || busy !== null) return;
    setBusy("translate");
    setFeedback(null);
    try {
      rememberTranslation(await requestTranslation(draft));
    } catch (reason) {
      setFeedback({ tone: "error", text: localizedError(reason, t, "chatbox.errors.translate") });
    } finally {
      setBusy(null);
    }
  }, [busy, draft, rememberTranslation, requestTranslation, t]);

  const send = useCallback(async () => {
    if (!draft.original.trim() || busy !== null) return false;
    let outgoing = draft;
    let stage: "translate" | "send" = "send";
    setFeedback(null);
    try {
      if (draft.send_mode !== "original" && !translationFresh) {
        stage = "translate";
        setBusy(stage);
        outgoing = await requestTranslation(draft);
        rememberTranslation(outgoing);
      }
      stage = "send";
      setBusy(stage);
      await coreApi.sendChatbox(outgoing);
      setDraft((current) => clearSentDraft(current));
      setTranslationBasis(null);
      return true;
    } catch (reason) {
      setFeedback({
        tone: "error",
        text: localizedError(
          reason,
          t,
          stage === "translate" ? "chatbox.errors.translate" : "chatbox.errors.send",
        ),
      });
      return false;
    } finally {
      setBusy(null);
    }
  }, [busy, draft, rememberTranslation, requestTranslation, t, translationFresh]);

  return {
    open,
    draft,
    setDraft: changeDraft,
    preview,
    busy,
    feedback,
    translationStale: Boolean(draft.translation?.trim()) && !translationFresh,
    show,
    close,
    translate,
    send,
  };
}
