export function prependScrollAdjustment(
  anchorOffset: number | null,
  currentOffset: number | null,
  previousScrollHeight: number,
  currentScrollHeight: number,
): number {
  if (anchorOffset !== null) {
    return currentOffset === null ? 0 : currentOffset - anchorOffset;
  }
  return Math.max(0, currentScrollHeight - previousScrollHeight);
}
