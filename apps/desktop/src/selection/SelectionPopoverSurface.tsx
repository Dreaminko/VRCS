import { useEffect, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";

import type { SelectionTarget } from "../app/app-types";
import {
  interfaceLayoutPixels,
  readAppliedInterfaceScaleFactor,
} from "../app/interface-scale";
import {
  isLookupAnchorVisible,
  placeLookupPopover,
} from "../shared/lib/popover-placement";

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
  const scale = compact ? 1 : readAppliedInterfaceScaleFactor();
  const viewportWidth = interfaceLayoutPixels(window.innerWidth, scale);
  const viewportHeight = interfaceLayoutPixels(window.innerHeight, scale);
  const layoutAnchor = compact ? anchor : {
    top: interfaceLayoutPixels(anchor.top, scale),
    bottom: interfaceLayoutPixels(anchor.bottom, scale),
    centerX: interfaceLayoutPixels(anchor.centerX, scale),
  };
  const panelWidth = Math.min(width, viewportWidth - 24);
  const placement = placeLookupPopover({
    anchor: layoutAnchor,
    popoverHeight: height,
    viewportHeight,
    viewportTop: 40,
  });
  const left = Math.min(
    Math.max(12, layoutAnchor.centerX - panelWidth / 2),
    viewportWidth - panelWidth - 12,
  );
  const arrowLeft = Math.min(
    Math.max(22, layoutAnchor.centerX - left - 8),
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

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    const closeOnOutside = (event: PointerEvent) => {
      if (!compact && ref.current && !ref.current.contains(event.target as Node)) onClose();
    };
    document.addEventListener("keydown", closeOnEscape);
    document.addEventListener("pointerdown", closeOnOutside);
    return () => {
      document.removeEventListener("keydown", closeOnEscape);
      document.removeEventListener("pointerdown", closeOnOutside);
    };
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
