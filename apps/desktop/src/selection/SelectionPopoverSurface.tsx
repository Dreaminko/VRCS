import { useEffect, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";

import type { SelectionTarget } from "../app/app-types";
import {
  isLookupAnchorVisible,
  placeLookupPopover,
} from "../shared/lib/popover-placement";
import { useDismissibleLayer } from "../shared/hooks/use-dismissible-layer";

export function SelectionPopoverSurface({
  target,
  compact = false,
  className,
  width,
  height,
  label,
  onClose,
  children,
}: {
  target: SelectionTarget;
  compact?: boolean;
  className: string;
  width: number;
  height: number;
  label: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [anchor, setAnchor] = useState(target.anchor);
  const viewportWidth = window.innerWidth;
  const viewportHeight = window.innerHeight;
  const panelWidth = Math.min(width, viewportWidth - 24);
  const placement = placeLookupPopover({
    anchor,
    popoverHeight: height,
    viewportHeight,
    viewportTop: 40,
  });
  const left = Math.min(
    Math.max(12, anchor.centerX - panelWidth / 2),
    viewportWidth - panelWidth - 12,
  );
  const arrowLeft = Math.min(
    Math.max(22, anchor.centerX - left - 8),
    panelWidth - 38,
  );
  const style = compact ? undefined : {
    left,
    top: placement.top,
    width: panelWidth,
    height: placement.height,
    "--arrow-left": `${arrowLeft}px`,
  };

  useEffect(() => {
    setAnchor(target.anchor);
  }, [target]);

  useEffect(() => {
    if (compact) return;
    const updateAnchor = () => {
      const rect = target.range.getBoundingClientRect();
      if (!isLookupAnchorVisible(rect, window.innerWidth, window.innerHeight, 40)) {
        onClose();
        return;
      }
      setAnchor({
        top: rect.top,
        bottom: rect.bottom,
        centerX: rect.left + rect.width / 2,
      });
    };
    updateAnchor();
    window.addEventListener("scroll", updateAnchor, true);
    window.addEventListener("resize", updateAnchor);
    return () => {
      window.removeEventListener("scroll", updateAnchor, true);
      window.removeEventListener("resize", updateAnchor);
    };
  }, [compact, onClose, target.range]);

  useDismissibleLayer(!compact, ref, onClose);

  useEffect(() => {
    if (!compact) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", closeOnEscape);
    return () => document.removeEventListener("keydown", closeOnEscape);
  }, [compact, onClose]);

  return (
    <div
      ref={ref}
      className={`selection-popover-surface ${className} ${compact ? "compact-inline-selection" : `popover-${placement.side}`}`}
      style={style as CSSProperties}
      role="dialog"
      aria-label={label}
    >
      {children}
      {!compact && <i className="popover-arrow" aria-hidden="true" />}
    </div>
  );
}
