export type Point = { x: number; y: number };
export type Region = { left: number; top: number; width: number; height: number };

/** Bitmap coordinates, independent of compositor scaling and logical screen origins. */
export function imageRegion(start: Point, end: Point, width: number, height: number): Region | null {
  if (![start.x, start.y, end.x, end.y, width, height].every(Number.isFinite) || width <= 0 || height <= 0) return null;
  const left = Math.max(0, Math.min(width, Math.floor(Math.min(start.x, end.x))));
  const top = Math.max(0, Math.min(height, Math.floor(Math.min(start.y, end.y))));
  const right = Math.max(0, Math.min(width, Math.ceil(Math.max(start.x, end.x))));
  const bottom = Math.max(0, Math.min(height, Math.ceil(Math.max(start.y, end.y))));
  return right - left >= 4 && bottom - top >= 4 ? { left, top, width: right - left, height: bottom - top } : null;
}
