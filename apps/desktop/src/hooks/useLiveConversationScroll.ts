import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { UIEvent } from "react";

import type { Page } from "../app-types";
import { shouldFollowLiveScroll } from "../live-scroll";

export function useLiveConversationScroll({
  page,
  running,
  activeConversationId,
  selectedConversationId,
  openedConversationId,
  loadingConversationSubtitles,
  selectedConversationUpdatedAt,
}: {
  page: Page;
  running: boolean;
  activeConversationId: string | null;
  selectedConversationId: string | null;
  openedConversationId: string | null;
  loadingConversationSubtitles: boolean;
  selectedConversationUpdatedAt: string | null;
}) {
  const liveScrollRef = useRef<HTMLDivElement>(null);
  const previousLiveScrollTopRef = useRef(0);
  const [followingLiveSubtitles, setFollowingLiveSubtitles] = useState(true);
  const liveAutoScrollActive = page === "live"
    && running
    && selectedConversationId !== null
    && selectedConversationId === activeConversationId;

  const scrollLiveViewToBottom = useCallback(
    (behavior: ScrollBehavior = "smooth") => {
      const scrollRegion = liveScrollRef.current;
      if (!scrollRegion) return;
      setFollowingLiveSubtitles(true);
      previousLiveScrollTopRef.current = scrollRegion.scrollTop;
      scrollRegion.scrollTo({ top: scrollRegion.scrollHeight, behavior });
    },
    [],
  );

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
    ) return;
    setFollowingLiveSubtitles(true);
    const frame = window.requestAnimationFrame(
      () => scrollLiveViewToBottom("auto"),
    );
    return () => window.cancelAnimationFrame(frame);
  }, [
    loadingConversationSubtitles,
    openedConversationId,
    page,
    scrollLiveViewToBottom,
    selectedConversationId,
  ]);

  useEffect(() => {
    if (!liveAutoScrollActive || !followingLiveSubtitles) return;
    const frame = window.requestAnimationFrame(
      () => scrollLiveViewToBottom(),
    );
    return () => window.cancelAnimationFrame(frame);
  }, [
    followingLiveSubtitles,
    liveAutoScrollActive,
    scrollLiveViewToBottom,
    selectedConversationUpdatedAt,
  ]);

  const onLiveScroll = useCallback((event: UIEvent<HTMLDivElement>) => {
    if (page !== "live") return;
    const scrollRegion = event.currentTarget;
    setFollowingLiveSubtitles((current) => shouldFollowLiveScroll(current, {
      scrollTop: scrollRegion.scrollTop,
      previousScrollTop: previousLiveScrollTopRef.current,
      scrollHeight: scrollRegion.scrollHeight,
      clientHeight: scrollRegion.clientHeight,
    }));
    previousLiveScrollTopRef.current = scrollRegion.scrollTop;
  }, [page]);

  return {
    liveScrollRef,
    followingLiveSubtitles,
    scrollLiveViewToBottom,
    onLiveScroll,
  };
}
