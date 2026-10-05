export const LIVE_SCROLL_BOTTOM_THRESHOLD = 48;

export type LiveScrollSnapshot = {
  scrollTop: number;
  previousScrollTop: number;
  scrollHeight: number;
  clientHeight: number;
};

export function shouldFollowLiveScroll(
  currentlyFollowing: boolean,
  snapshot: LiveScrollSnapshot,
): boolean {
  // Shorter final text or a taller viewport can clamp the browser's offset.
  // Only movement above that clamped position means the reader scrolled up.
  const maximumScrollTop = Math.max(0, snapshot.scrollHeight - snapshot.clientHeight);
  const previousScrollTop = Math.min(snapshot.previousScrollTop, maximumScrollTop);
  if (snapshot.scrollTop < previousScrollTop - 1) {
    return false;
  }

  if (snapshot.previousScrollTop > maximumScrollTop + 1) {
    return currentlyFollowing;
  }

  const distanceFromBottom = maximumScrollTop - snapshot.scrollTop;
  if (distanceFromBottom <= LIVE_SCROLL_BOTTOM_THRESHOLD) {
    return true;
  }

  return currentlyFollowing;
}
