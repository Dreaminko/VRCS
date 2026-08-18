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
  if (snapshot.scrollTop < snapshot.previousScrollTop - 1) {
    return false;
  }

  const distanceFromBottom = snapshot.scrollHeight - snapshot.clientHeight - snapshot.scrollTop;
  if (distanceFromBottom <= LIVE_SCROLL_BOTTOM_THRESHOLD) {
    return true;
  }

  return currentlyFollowing;
}
