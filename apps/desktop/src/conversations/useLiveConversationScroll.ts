import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { UIEvent } from "react";

import type { Page } from "../app/app-types";
import { shouldFollowLiveScroll } from "../shared/lib/live-scroll";

export function useLiveConversationScroll({
  page,
  running,
  activeConversationId,
  selectedConversationId,
  openedConversationId,
  loadingConversationSubtitles,
  selectedConversationUpdatedAt,
  focusedSubtitleId,
}: {
  page: Page;
  running: boolean;
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
    && running
    && focusedSubtitleId === null
    && selectedConversationId !== null
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
      previousLiveScrollTopRef.current = scrollRegion.scrollTop;
      scrollRegion.scrollTo({ top: scrollRegion.scrollHeight, behavior });
    },
    [setFollowingLive],
  );

  const autoScrollLiveViewToBottom = useCallback(() => {
    if (!followingLiveSubtitlesRef.current) return;
    const scrollRegion = liveScrollRef.current;
    if (!scrollRegion) return;
    previousLiveScrollTopRef.current = scrollRegion.scrollTop;
    scrollRegion.scrollTo({ top: scrollRegion.scrollHeight, behavior: "auto" });
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
    const frame = window.requestAnimationFrame(
      autoScrollLiveViewToBottom,
    );
    return () => window.cancelAnimationFrame(frame);
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
