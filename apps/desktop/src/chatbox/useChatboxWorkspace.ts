import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { subtitlesApi } from "../subtitles/api";
import { chatboxApi } from "./api";
import { localizedError } from "../app/app-utils";
import {
  type ChatboxDraft,
  applyChatboxPreferences,
  chatboxPreferencesFromDraft,
  clearSentDraft,
  createChatboxDraft,
  previewChatboxLocally,
} from "./chatbox";
import {
  loadChatboxPreferences,
  saveChatboxPreferences,
} from "./chatbox-preferences";
import type { ActiveLanguageSession } from "../core-client/types";
import type {
  ChatboxComposeInput,
  ChatboxPreview,
} from "./types";
import type { Settings } from "../settings/types";

export function useChatboxWorkspace(
  settings: Settings | null,
  roomScopeId: string | null = null,
  languageSession?: ActiveLanguageSession,
) {
  const { t } = useTranslation();
  const available = settings !== null;
  const availableRef = useRef(available);
  availableRef.current = available;
  const roomScopeRef = useRef(roomScopeId);
  roomScopeRef.current = roomScopeId;
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState<ChatboxDraft>(() => createChatboxDraft());
  const draftRef = useRef(draft);
  draftRef.current = draft;
  const [translationBasis, setTranslationBasis] = useState<{
    original: string;
    targetLanguage: string | null;
    roomScopeId: string | null;
  } | null>(null);
  const effectiveTranslation = languageSession && languageSession.kind !== "global"
    ? languageSession.translation
    : settings?.translation;
  const preferredTarget = effectiveTranslation?.microphone_targets[0]?.target_language ?? "ja";
  const oscEnabled = Boolean(settings?.osc?.enabled);
  const unitextEnabled = Boolean(settings?.unitext?.enabled);
  const sendOsc = oscEnabled && (!unitextEnabled || draft.output_target !== "unitext");
  const sendUnitext = unitextEnabled && (!oscEnabled || draft.output_target !== "osc");
  const enforceOscLimit = sendOsc || !sendUnitext;
  const oscMessageFormat = settings?.osc?.message_format ?? "original_newline_translation";
  const oscCustomFormat = settings?.osc?.custom_format ?? null;
  const resolvedDraft = useMemo(() => {
    const { follow_translation_settings, ...input } = draft;
    return {
      ...input,
      target_language: follow_translation_settings ? preferredTarget : input.target_language,
      message_format: oscMessageFormat,
      custom_format: oscMessageFormat === "custom" ? oscCustomFormat : null,
    };
  }, [draft, oscCustomFormat, oscMessageFormat, preferredTarget]);
  const targetRef = useRef(resolvedDraft.target_language);
  targetRef.current = resolvedDraft.target_language;
  const localPreview = useMemo(
    () => previewChatboxLocally(resolvedDraft, { enforceOscLimit }),
    [enforceOscLimit, resolvedDraft],
  );
  const [remotePreview, setRemotePreview] = useState<{
    draft: ChatboxComposeInput;
    enforceOscLimit: boolean;
    value: ChatboxPreview;
  } | null>(null);
  const preview = remotePreview?.draft === resolvedDraft && remotePreview.enforceOscLimit === enforceOscLimit
    ? remotePreview.value
    : localPreview;
  const [busy, setBusy] = useState<"translate" | "send" | null>(null);
  const [feedback, setFeedback] = useState<{ tone: "success" | "error"; text: string } | null>(null);
  const previewRequest = useRef(0);
  const preferencesRequested = useRef(false);
  const [preferencesReady, setPreferencesReady] = useState(false);
  const translationFresh = Boolean(draft.translation?.trim())
    && translationBasis?.original === draft.original
    && translationBasis.targetLanguage === resolvedDraft.target_language
    && translationBasis.roomScopeId === roomScopeId;

  useEffect(() => {
    if (!settings || preferencesRequested.current) return;
    preferencesRequested.current = true;
    void loadChatboxPreferences(preferredTarget).then((preferences) => {
      setDraft((current) => {
        const next = applyChatboxPreferences(current, preferences);
        draftRef.current = next;
        return next;
      });
      setPreferencesReady(true);
    });
  }, [settings, preferredTarget]);

  useEffect(() => {
    const requestId = ++previewRequest.current;
    if (!available || !preferencesReady || !open || (draft.send_mode !== "original" && !translationFresh)) return;
    const timer = window.setTimeout(() => {
      void chatboxApi.previewChatbox(resolvedDraft).then(
        (value) => {
          if (requestId === previewRequest.current) {
            setRemotePreview({ draft: resolvedDraft, enforceOscLimit, value });
          }
        },
        () => undefined,
      );
    }, 180);
    return () => window.clearTimeout(timer);
  }, [available, preferencesReady, open, draft.send_mode, resolvedDraft, translationFresh, enforceOscLimit]);

  const changeDraft = useCallback((next: ChatboxDraft) => {
    if (!preferencesReady) return;
    const current = draftRef.current;
    if (next.translation !== current.translation) {
      setTranslationBasis(next.translation?.trim()
        ? {
            original: next.original,
            targetLanguage: next.follow_translation_settings ? preferredTarget : next.target_language,
            roomScopeId: roomScopeRef.current,
          }
        : null);
    }
    const currentPreferences = chatboxPreferencesFromDraft(current);
    const nextPreferences = chatboxPreferencesFromDraft(next);
    if (
      currentPreferences.follow_translation_settings !== nextPreferences.follow_translation_settings
      || currentPreferences.target_language !== nextPreferences.target_language
      || currentPreferences.send_mode !== nextPreferences.send_mode
      || currentPreferences.output_target !== nextPreferences.output_target
      || currentPreferences.overflow_policy !== nextPreferences.overflow_policy
    ) {
      void saveChatboxPreferences(nextPreferences);
    }
    draftRef.current = next;
    setDraft(next);
    setFeedback(null);
  }, [preferredTarget, preferencesReady]);

  const show = useCallback(() => {
    if (!availableRef.current) return;
    setOpen(true);
    setFeedback(null);
  }, []);

  useEffect(() => {
    if (!available) {
      setOpen(false);
      ++previewRequest.current;
    }
  }, [available]);

  const close = useCallback(() => setOpen(false), []);

  const requestTranslation = useCallback(async <T extends ChatboxComposeInput>(input: T) => {
    const requestRoom = roomScopeRef.current;
    const result = await subtitlesApi.previewTranslation(
      input.original,
      input.source_language,
      input.target_language ?? undefined,
    );
    if (!availableRef.current) return null;
    if (
      roomScopeRef.current !== requestRoom
      || targetRef.current !== input.target_language
      || draftRef.current.original !== input.original
    ) {
      setFeedback({ tone: "error", text: t("chatbox.translationContextChanged") });
      return null;
    }
    return {
      ...input,
      translation: result.text,
      translation_origin: {
        provider: result.provider,
        model: result.model,
      },
      source_language: result.source_language,
      target_language: result.target_language,
    };
  }, [t]);

  const rememberTranslation = useCallback((input: ChatboxComposeInput) => {
    if (!availableRef.current) return;
    const current = draftRef.current;
    if (current.original !== input.original || targetRef.current !== input.target_language) {
      return;
    }
    const next = {
      ...current,
      translation: input.translation,
      translation_origin: input.translation_origin,
      source_language: input.source_language,
    };
    draftRef.current = next;
    setDraft(next);
    setTranslationBasis({
      original: input.original,
      targetLanguage: input.target_language,
      roomScopeId: roomScopeRef.current,
    });
  }, []);

  const translate = useCallback(async () => {
    if (!preferencesReady || !availableRef.current || !draft.original.trim() || busy !== null) return;
    setBusy("translate");
    setFeedback(null);
    try {
      const translation = await requestTranslation(resolvedDraft);
      if (translation) rememberTranslation(translation);
    } catch (reason) {
      setFeedback({ tone: "error", text: localizedError(reason, t, "chatbox.errors.translate") });
    } finally {
      setBusy(null);
    }
  }, [preferencesReady, busy, draft, resolvedDraft, rememberTranslation, requestTranslation, t]);

  const send = useCallback(async () => {
    if (!preferencesReady || !availableRef.current || !draft.original.trim() || busy !== null) return false;
    // The core also saves unused translations in send history.
    let outgoing = draft.send_mode === "original"
      ? { ...resolvedDraft, translation: null, translation_origin: null, source_language: null }
      : resolvedDraft;
    let stage: "translate" | "send" = "send";
    setFeedback(null);
    try {
      if (draft.send_mode !== "original" && !translationFresh) {
        stage = "translate";
        setBusy(stage);
        const translation = await requestTranslation(resolvedDraft);
        if (!translation) return false;
        outgoing = translation;
        rememberTranslation(outgoing);
      }
      if (!availableRef.current) return false;
      stage = "send";
      setBusy(stage);
      await chatboxApi.sendChatbox(outgoing);
      const cleared = clearSentDraft(draftRef.current);
      draftRef.current = cleared;
      setDraft(cleared);
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
  }, [preferencesReady, busy, draft, resolvedDraft, rememberTranslation, requestTranslation, t, translationFresh]);

  return {
    open: available && preferencesReady && open,
    draft,
    preferredTarget,
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
