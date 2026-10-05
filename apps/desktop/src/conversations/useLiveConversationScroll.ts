import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { UIEvent } from "react";

import type { Page } from "../app/app-types";
import { shouldFollowLiveScroll } from "../shared/lib/live-scroll";

export function useLiveConversationScroll({
  page,
  activeConversationId,
  selectedConversationId,
  openedConversationId,
  loadingConversationSubtitles,
  selectedConversationUpdatedAt,
  focusedSubtitleId,
}: {
  page: Page;
  activeConversationId: string | null;
  selectedConversationId: string | null;
  openedConversationId: string | null;
  loadingConversationSubtitles: boolean;
  selectedConversationUpdatedAt: string | null;
  focusedSubtitleId: number | null;
}) {
  const liveScrollRef = useRef<HTMLDivElement>(null);
  const previousLiveScrollTopRef = useRef(0);
  const followingLiveSubtitlesRef = useRef(true);
  const [followingLiveSubtitles, setFollowingLiveSubtitles] = useState(true);
  const liveAutoScrollActive = page === "live"
    && focusedSubtitleId === null
    && selectedConversationId !== null
    && openedConversationId === selectedConversationId
    && !loadingConversationSubtitles
    && selectedConversationId === activeConversationId;

  const setFollowingLive = useCallback((following: boolean) => {
    followingLiveSubtitlesRef.current = following;
    setFollowingLiveSubtitles(following);
  }, []);

  const scrollLiveViewToBottom = useCallback(
    (behavior: ScrollBehavior = "smooth") => {
      const scrollRegion = liveScrollRef.current;
      if (!scrollRegion) return;
      setFollowingLive(true);
      scrollRegion.scrollTo({ top: scrollRegion.scrollHeight, behavior });
      previousLiveScrollTopRef.current = scrollRegion.scrollTop;
    },
    [setFollowingLive],
  );

  const autoScrollLiveViewToBottom = useCallback(() => {
    if (!followingLiveSubtitlesRef.current) return;
    const scrollRegion = liveScrollRef.current;
    if (!scrollRegion) return;
    scrollRegion.scrollTo({ top: scrollRegion.scrollHeight, behavior: "auto" });
    previousLiveScrollTopRef.current = scrollRegion.scrollTop;
  }, []);

  useLayoutEffect(() => {
    if (page === "live") return;
    const scrollRegion = liveScrollRef.current;
    if (!scrollRegion) return;
    scrollRegion.scrollTop = 0;
    previousLiveScrollTopRef.current = 0;
  }, [page]);

  useEffect(() => {
    if (
      page !== "live"
      || selectedConversationId === null
      || openedConversationId !== selectedConversationId
      || loadingConversationSubtitles
      || focusedSubtitleId !== null
    ) return;
    setFollowingLive(true);
    const frame = window.requestAnimationFrame(autoScrollLiveViewToBottom);
    return () => window.cancelAnimationFrame(frame);
  }, [
    loadingConversationSubtitles,
    openedConversationId,
    page,
    selectedConversationId,
    focusedSubtitleId,
    setFollowingLive,
    autoScrollLiveViewToBottom,
  ]);

  useEffect(() => {
    if (focusedSubtitleId !== null) setFollowingLive(false);
  }, [focusedSubtitleId, setFollowingLive]);

  useEffect(() => {
    if (!liveAutoScrollActive || !followingLiveSubtitles) return;
    let frame = window.requestAnimationFrame(autoScrollLiveViewToBottom);
    // Delta updates grow the preview without changing the saved conversation.
    const observer = new ResizeObserver(() => {
      window.cancelAnimationFrame(frame);
      frame = window.requestAnimationFrame(autoScrollLiveViewToBottom);
    });
    const scrollRegion = liveScrollRef.current;
    if (scrollRegion) observer.observe(scrollRegion);
    const content = scrollRegion?.firstElementChild;
    if (content) observer.observe(content);
    return () => {
      observer.disconnect();
      window.cancelAnimationFrame(frame);
    };
  }, [
    autoScrollLiveViewToBottom,
    followingLiveSubtitles,
    liveAutoScrollActive,
    selectedConversationUpdatedAt,
  ]);

  const onLiveScroll = useCallback((event: UIEvent<HTMLDivElement>) => {
    if (page !== "live") return;
    const scrollRegion = event.currentTarget;
    setFollowingLive(shouldFollowLiveScroll(followingLiveSubtitlesRef.current, {
      scrollTop: scrollRegion.scrollTop,
      previousScrollTop: previousLiveScrollTopRef.current,
      scrollHeight: scrollRegion.scrollHeight,
      clientHeight: scrollRegion.clientHeight,
    }));
    previousLiveScrollTopRef.current = scrollRegion.scrollTop;
  }, [page, setFollowingLive]);

  return {
    liveScrollRef,
    followingLiveSubtitles,
    scrollLiveViewToBottom,
    onLiveScroll,
  };
}
