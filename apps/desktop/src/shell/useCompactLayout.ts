import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";

import type { LookupOrigin, SelectionTarget } from "../app/app-types";
import type { LiveTranscription } from "../capture/types";
import { useTranslationPartialsForSubtitles } from "../core-client/realtime-state";
import { useBatchedPreview } from "../shared/hooks/streaming-preview";
import type { SpeakerIdentity, Subtitle } from "../subtitles/types";
import { compactPartialMatchesSubtitle, compactPreviewText } from "./compact-mode";
import { latestCaptionsByTrack } from "./player-previews";
import {
  allocateLatestLines, COMPACT_FONT_PRESETS, fitRecentGroups,
  type CompactFontSize, type CompactTextBudget,
} from "./compact-layout";

export interface CompactGroup {
  speaker?: SpeakerIdentity | null;
  source?: string;
  live?: boolean;
  key: string;
  context: string;
  text: string;
  language?: string | null;
  translation: string;
  targetLanguage?: string | null;
  streaming: boolean;
  origin?: LookupOrigin;
}

interface LayoutSnapshot {
  parallel?: boolean;
  scrollable?: boolean;
  groups: CompactGroup[];
  latestBudget?: CompactTextBudget;
}

function sameGroups(left: CompactGroup[], right: CompactGroup[]) {
  return left.length === right.length && left.every((group, index) => {
    const other = right[index];
    return group.speaker?.id === other.speaker?.id && group.speaker?.display_name === other.speaker?.display_name
      && group.live === other.live && group.source === other.source && group.key === other.key && group.text === other.text && group.context === other.context
      && group.language === other.language && group.translation === other.translation
      && group.targetLanguage === other.targetLanguage && group.streaming === other.streaming
      && group.origin?.id === other.origin?.id && group.origin?.source === other.origin?.source
      && group.origin?.createdAt === other.origin?.createdAt
      && group.origin?.translation === other.origin?.translation;
  });
}

function hasContentSelection(element: HTMLElement | null) {
  const selection = window.getSelection();
  return Boolean(element && selection && !selection.isCollapsed
    && (element.contains(selection.anchorNode) || element.contains(selection.focusNode)));
}

function matchesSelectionTarget(group: CompactGroup, target: SelectionTarget) {
  if (target.origin?.id != null) return group.origin?.id === target.origin.id;
  if (target.origin) {
    return group.origin?.createdAt === target.origin.createdAt
      && group.origin?.source === target.origin.source && group.context === target.context;
  }
  return !group.origin && group.context === target.context;
}

function useCandidateGroups(subtitles: Subtitle[], previews: LiveTranscription[], limit: number, perPlayer: boolean) {
  const lives = useMemo(() => previews.filter(partial => partial.text.trimEnd() || partial.translation?.trimEnd()), [previews]);
  const candidates = useMemo(() => {
    const history = subtitles.filter((subtitle) => !lives.some(live => compactPartialMatchesSubtitle(live, subtitle))
      && (subtitle.text.trimEnd() || subtitle.translations[0]?.text.trimEnd()
        || subtitle.translation_partial?.text.trimEnd()));
    // Bound history by track before measuring, so one busy player cannot push
    // another player's completed translation out of the candidate window.
    return perPlayer ? latestCaptionsByTrack(history, limit) : history.slice(-limit);
  }, [subtitles, lives, limit, perPlayer]);
  const ids = useMemo(() => candidates.map((subtitle) => subtitle.id), [candidates]);
  const partials = useTranslationPartialsForSubtitles(ids);
  const qwenPartials = useMemo(() => candidates.flatMap((subtitle, index) => {
    const value = partials[index][0] ?? subtitle.translation_partial;
    return value && subtitle.utterance_id?.includes("qwen-source-")
      ? [{ id: subtitle.id, value }] : [];
  }), [candidates, partials]);
  const batched = useBatchedPreview(qwenPartials, "compact-translations", qwenPartials.length > 0);

  return useMemo(() => {
    const groups: CompactGroup[] = candidates.map((subtitle, index) => {
      const pending = partials[index][0] ?? subtitle.translation_partial;
      const preview = subtitle.utterance_id?.includes("qwen-source-") && pending
        ? batched?.find((item) => item.id === subtitle.id
          && item.value.target_language === pending.target_language)?.value
        : pending;
      const translation = preview ?? subtitle.translations[0];
      return {
        key: `subtitle:${subtitle.id ?? `${subtitle.source}:${subtitle.created_at}`}`,
        speaker: subtitle.speaker, source: subtitle.source,
        context: subtitle.text,
        text: compactPreviewText(subtitle.text),
        language: subtitle.language,
        translation: compactPreviewText(translation?.text ?? ""),
        targetLanguage: translation?.target_language,
        streaming: Boolean(preview),
        origin: {
          id: subtitle.id, language: subtitle.language, source: subtitle.source ?? null,
          createdAt: subtitle.created_at, translation: translation?.text ?? null,
        },
      };
    });
    for (const live of lives) {
      const completed = subtitles.find((subtitle) => compactPartialMatchesSubtitle(live, subtitle));
      const group: CompactGroup = {
        key: completed
          ? `subtitle:${completed.id ?? `${completed.source}:${completed.created_at}`}`
          : `partial:${live.source}:${live.source_utterance_id ?? live.utterance_id}`,
        speaker: live.speaker, source: live.source, live: true,
        context: live.text,
        text: compactPreviewText(live.text),
        language: live.language,
        translation: compactPreviewText(live.translation ?? ""),
        targetLanguage: live.target_language,
        streaming: false,
        origin: completed ? {
          id: completed.id, language: completed.language, source: completed.source ?? null,
          createdAt: completed.created_at, translation: live.translation ?? null,
        } : undefined,
      };
      // An older speech preview must not hide a message sent from Chatbox.
      const receivedAt = live.received_at;
      const newerChatbox = !perPlayer && receivedAt !== undefined
        ? groups.findIndex((item) => item.source === "chatbox"
          && Date.parse(item.origin?.createdAt ?? "") >= receivedAt)
        : -1;
      if (newerChatbox < 0) groups.push(group);
      else groups.splice(newerChatbox, 0, group);
    }
    const visible = groups.filter((group) => group.text || group.translation);
    return perPlayer ? latestCaptionsByTrack(visible) : visible;
  }, [candidates, partials, batched, lives, subtitles, perPlayer]);
}

export function useCompactLayout({ subtitles, previews, fontSize, selectionTarget, transparentBackground, perPlayer = false }: {
  subtitles: Subtitle[];
  previews: LiveTranscription[];
  fontSize: CompactFontSize;
  selectionTarget: SelectionTarget | null;
  transparentBackground: boolean;
  perPlayer?: boolean;
}) {
  const contentRef = useRef<HTMLDivElement>(null);
  const measureRef = useRef<HTMLDivElement>(null);
  const [availableHeight, setAvailableHeight] = useState(window.innerHeight);
  const preset = COMPACT_FONT_PRESETS[fontSize];
  // A nonempty group takes at least one line. This only bounds DOM work.
  const limit = Math.ceil(availableHeight / Math.min(preset.originalLine, preset.translationLine)) + 1;
  const candidates = useCandidateGroups(subtitles, previews, limit, perPlayer);
  const [selectionFrozen, setSelectionFrozen] = useState(false);
  const frozen = Boolean(selectionTarget) || selectionFrozen;
  const frozenGroups = useRef<CompactGroup[] | null>(null);
  const committedGroups = useRef<CompactGroup[]>([]);
  const freezeRef = useRef(false);
  const [layout, setLayout] = useState<LayoutSnapshot>({ groups: [] });
  // Dragging freezes the visible list; a captured target pins just that group's
  // displayed text, including previews which may no longer exist in history.
  const selectedGroups = useMemo(() => {
    if (!selectionTarget) return null;
    const snapshot = frozenGroups.current ?? committedGroups.current;
    const selected = snapshot.find((group) => matchesSelectionTarget(group, selectionTarget))
      ?? committedGroups.current.find((group) => matchesSelectionTarget(group, selectionTarget));
    return [selected ?? {
      key: `selection:${selectionTarget.origin?.id ?? selectionTarget.origin?.createdAt ?? selectionTarget.context}`,
      context: selectionTarget.context,
      text: compactPreviewText(selectionTarget.context),
      language: selectionTarget.origin?.language,
      translation: compactPreviewText(selectionTarget.origin?.translation ?? ""),
      streaming: false,
      origin: selectionTarget.origin,
    }];
  }, [selectionTarget]);
  const groups = selectedGroups ?? (frozen ? frozenGroups.current ?? committedGroups.current : candidates);
  const frameRef = useRef<number | null>(null);
  const geometryRef = useRef("");
  const measuredSelectionRef = useRef<SelectionTarget | null>(null);

  const freezeSelection = useCallback(() => {
    if (!frozenGroups.current) frozenGroups.current = committedGroups.current;
    freezeRef.current = true;
    setSelectionFrozen(true);
  }, []);

  useLayoutEffect(() => {
    if (frozen) frozenGroups.current ??= committedGroups.current;
    else frozenGroups.current = null;
    freezeRef.current = frozen;
  }, [frozen]);
  useLayoutEffect(() => {
    committedGroups.current = layout.groups;
  }, [layout.groups]);

  useLayoutEffect(() => {
    let dragging = false;
    const start = (event: PointerEvent) => {
      if (event.button !== 0 || !(event.target instanceof Element)
        || !event.target.closest(".compact-original")
        || !contentRef.current?.contains(event.target)) return;
      dragging = true;
      freezeSelection();
    };
    const update = () => {
      if (dragging) return;
      if (hasContentSelection(contentRef.current)) freezeSelection();
      else setSelectionFrozen(false);
    };
    const end = () => { dragging = false; update(); };
    document.addEventListener("pointerdown", start);
    document.addEventListener("pointerup", end);
    document.addEventListener("pointercancel", end);
    document.addEventListener("selectionchange", update);
    window.addEventListener("blur", end);
    return () => {
      document.removeEventListener("pointerdown", start);
      document.removeEventListener("pointerup", end);
      document.removeEventListener("pointercancel", end);
      document.removeEventListener("selectionchange", update);
      window.removeEventListener("blur", end);
    };
  }, [freezeSelection]);

  useLayoutEffect(() => {
    const viewport = contentRef.current;
    const measure = measureRef.current;
    if (!viewport || !measure) return;
    const reflow = () => {
      frameRef.current = null;
      const width = viewport.clientWidth;
      const height = viewport.clientHeight;
      setAvailableHeight((previous) => previous === height ? previous : height);
      if (!width || !height) return;
      const geometry = `${width}:${height}:${fontSize}:${transparentBackground}`;
      if ((frozen || freezeRef.current) && geometryRef.current === geometry
        && measuredSelectionRef.current === selectionTarget) return;
      const rows = Array.from(measure.children) as HTMLElement[];
      if (rows.length !== groups.length) return;
      const chatboxStart = groups.findIndex(group => group.source === "chatbox");
      const scrollable = !frozen && chatboxStart >= 0;
      geometryRef.current = geometry;
      measuredSelectionRef.current = selectionTarget;
      if (!frozen && perPlayer && chatboxStart !== 0 && groups.length > 1) {
        const budget = { original: preset.originalLine, translation: preset.translationLine };
        setLayout(previous => sameGroups(previous.groups, groups) && previous.parallel === true && previous.scrollable === scrollable
          && previous.latestBudget?.original === budget.original && previous.latestBudget?.translation === budget.translation
          ? previous : { groups, latestBudget: budget, parallel: true, scrollable });
        return;
      }
      const heights = rows.map((row) => row.getBoundingClientRect().height);
      let count = fitRecentGroups(heights, height);
      let latestBudget: CompactTextBudget | undefined;
      if (!count && groups.length) {
        const latest = rows.at(-1)!;
        const original = latest.querySelector<HTMLElement>(".compact-original");
        const translation = latest.querySelector<HTMLElement>(".compact-translation");
        latestBudget = allocateLatestLines({
          original: original?.getBoundingClientRect().height ?? 0,
          translation: translation?.getBoundingClientRect().height ?? 0,
        }, Math.max(0, height - (latest.querySelector<HTMLElement>(".compact-speaker")?.getBoundingClientRect().height ?? 0) - 4), fontSize);
        count = 1;
      }
      // Keep bounded Chatbox history in the scroll viewport, even when only part
      // of the previous message fits above the newest one.
      const nextGroups = scrollable ? groups : count ? groups.slice(-count) : [];
      setLayout((previous) => sameGroups(previous.groups, nextGroups) && !previous.parallel
        && previous.scrollable === scrollable
        && previous.latestBudget?.original === latestBudget?.original
        && previous.latestBudget?.translation === latestBudget?.translation
        ? previous : { groups: nextGroups, latestBudget, scrollable });
    };
    const schedule = () => {
      if (frameRef.current === null) frameRef.current = requestAnimationFrame(reflow);
    };
    const observer = new ResizeObserver(schedule);
    observer.observe(viewport);
    observer.observe(measure);
    let disposed = false;
    // Font changes can affect wrapping even when the measurement layer's total height is unchanged.
    const fontsChanged = () => {
      if (disposed) return;
      geometryRef.current = "";
      schedule();
    };
    void document.fonts.ready.then(fontsChanged);
    document.fonts.addEventListener("loadingdone", fontsChanged);
    // Commit the selected group before paint, even when native resizing has not
    // changed the viewport yet. A frozen list must never hide the lookup target.
    if (measuredSelectionRef.current !== selectionTarget) reflow();
    else schedule();
    return () => {
      disposed = true;
      observer.disconnect();
      document.fonts.removeEventListener("loadingdone", fontsChanged);
      if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
      frameRef.current = null;
    };
  }, [groups, fontSize, frozen, selectionTarget, transparentBackground, perPlayer]);

  return {
    contentRef, measureRef, measurementGroups: groups,
    visibleGroups: layout.groups, parallel: layout.parallel, scrollable: layout.scrollable, latestBudget: layout.latestBudget, frozen,
  };
}
