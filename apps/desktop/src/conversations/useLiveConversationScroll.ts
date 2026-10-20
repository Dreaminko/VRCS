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

  useLayoutEffect(() => {
    if (
      page !== "live"
      || selectedConversationId === null
      || openedConversationId !== selectedConversationId
      || loadingConversationSubtitles
      || focusedSubtitleId !== null
    ) return;
    setFollowingLive(true);
    autoScrollLiveViewToBottom();
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
    if (page !== "live") return;
    const scrollRegion = liveScrollRef.current;
    if (!scrollRegion) return;

    const pauseFollowing = () => {
      if (scrollRegion.scrollTop > 0) setFollowingLive(false);
    };
    const onWheel = (event: WheelEvent) => {
      if (!event.defaultPrevented && !event.ctrlKey && event.deltaY < 0) pauseFollowing();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.altKey || event.metaKey) return;
      const target = event.target instanceof Element ? event.target : null;
      if (target && target !== document.body && target !== document.documentElement && !scrollRegion.contains(target)) return;
      if (target?.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"]), [role="textbox"]')) return;
      if (event.ctrlKey && event.key !== "Home") return;
      const reverseSpace = event.key === " " && event.shiftKey
        && !target?.closest('button, a[href], [role="button"]');
      if (event.key === "ArrowUp" || event.key === "PageUp" || event.key === "Home" || reverseSpace) pauseFollowing();
    };
    const onPointerDown = (event: PointerEvent) => {
      if (event.button !== 0 || event.target !== scrollRegion) return;
      const bounds = scrollRegion.getBoundingClientRect();
      const scale = bounds.width / scrollRegion.offsetWidth;
      const contentLeft = bounds.left + scrollRegion.clientLeft * scale;
      const contentRight = contentLeft + scrollRegion.clientWidth * scale;
      if (event.clientX < contentLeft || event.clientX >= contentRight) pauseFollowing();
    };
    let touch: { x: number; y: number } | null = null;
    const onTouchStart = (event: TouchEvent) => {
      const point = event.touches.length === 1 ? event.touches[0] : null;
      touch = point ? { x: point.clientX, y: point.clientY } : null;
    };
    const onTouchMove = (event: TouchEvent) => {
      const point = event.touches.length === 1 ? event.touches[0] : null;
      if (touch && point && !event.defaultPrevented) {
        const deltaY = point.clientY - touch.y;
        if (deltaY > 1 && deltaY > Math.abs(point.clientX - touch.x)) pauseFollowing();
      }
      touch = point ? { x: point.clientX, y: point.clientY } : null;
    };
    const onTouchEnd = () => { touch = null; };

    scrollRegion.addEventListener("wheel", onWheel, { passive: true });
    scrollRegion.addEventListener("pointerdown", onPointerDown);
    scrollRegion.addEventListener("touchstart", onTouchStart, { passive: true });
    scrollRegion.addEventListener("touchmove", onTouchMove, { passive: true });
    scrollRegion.addEventListener("touchend", onTouchEnd);
    scrollRegion.addEventListener("touchcancel", onTouchEnd);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      scrollRegion.removeEventListener("wheel", onWheel);
      scrollRegion.removeEventListener("pointerdown", onPointerDown);
      scrollRegion.removeEventListener("touchstart", onTouchStart);
      scrollRegion.removeEventListener("touchmove", onTouchMove);
      scrollRegion.removeEventListener("touchend", onTouchEnd);
      scrollRegion.removeEventListener("touchcancel", onTouchEnd);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [page, setFollowingLive]);

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
    // Layout can clamp the offset before another translation grows the content.
    // Input pauses following; only movement back toward the bottom resumes it.
    if (!followingLiveSubtitlesRef.current && scrollRegion.scrollTop > previousLiveScrollTopRef.current) {
      setFollowingLive(shouldFollowLiveScroll(false, {
        scrollTop: scrollRegion.scrollTop,
        previousScrollTop: previousLiveScrollTopRef.current,
        scrollHeight: scrollRegion.scrollHeight,
        clientHeight: scrollRegion.clientHeight,
      }));
    }
    previousLiveScrollTopRef.current = scrollRegion.scrollTop;
  }, [page, setFollowingLive]);

  return {
    liveScrollRef,
    followingLiveSubtitles,
    scrollLiveViewToBottom,
    onLiveScroll,
  };
}
