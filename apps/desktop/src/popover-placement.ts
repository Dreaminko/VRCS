export type LookupAnchor = { top: number; bottom: number; centerX: number };

type PlacementInput = {
  anchor: LookupAnchor;
  popoverHeight: number;
  viewportHeight: number;
  viewportTop?: number;
  margin?: number;
  gap?: number;
};

export function placeLookupPopover({
  anchor,
  popoverHeight,
  viewportHeight,
  viewportTop = 40,
  margin = 12,
  gap = 10,
}: PlacementInput) {
  const availableAbove = Math.max(0, anchor.top - gap - viewportTop - margin);
  const availableBelow = Math.max(0, viewportHeight - margin - anchor.bottom - gap);
  const side = availableBelow >= popoverHeight || availableBelow >= availableAbove ? "below" : "above";
  const maxHeight = side === "below" ? availableBelow : availableAbove;
  const visibleHeight = Math.min(popoverHeight, maxHeight);
  const top = side === "below" ? anchor.bottom + gap : anchor.top - gap - visibleHeight;

  return { side, top, maxHeight } as const;
}
