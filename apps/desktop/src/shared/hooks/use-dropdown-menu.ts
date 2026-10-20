import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { CSSProperties, RefObject } from "react";

import {
  INTERFACE_LAYOUT_CHANGE_EVENT,
  interfaceLayoutPixels,
  readAppliedInterfaceScaleFactor,
} from "../../app/interface-scale";
import { placeLookupPopover } from "../lib/popover-placement";
import { useDropdownMenuMotion } from "./use-dropdown-menu-motion";

export type FloatingMenuPosition = Pick<CSSProperties, "bottom" | "left" | "maxHeight" | "right" | "top" | "width">;

const FLOATING_MENU_GAP = 6;
const FLOATING_MENU_MARGIN = 12;
const FLOATING_MENU_MAX_HEIGHT = 216;

// Owns the dropdown menu ref/motion plus optional fixed-position placement.
// Floating menus are portaled by the caller so they can escape ancestor
// clipping and stacking contexts (for example the overflow on
// `::details-content`). The ready flag handed to the motion hook keeps the
// open animation pending until the position has been computed.
export function useDropdownMenu(open: boolean, {
  floating,
  floatingWidth,
  anchorRef,
  onClose,
}: {
  floating: boolean;
  floatingWidth?: number;
  anchorRef: RefObject<HTMLElement | null>;
  onClose: () => void;
}) {
  const [floatingPosition, setFloatingPosition] = useState<FloatingMenuPosition | null>(null);
  const [floatingSide, setFloatingSide] = useState<"above" | "below">("below");
  const { menuRef, mounted: menuMounted } = useDropdownMenuMotion(open, !floating || floatingPosition !== null);
  const onCloseRef = useRef(onClose);
  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  const updateFloatingPosition = useCallback(() => {
    if (!floating || !open || !anchorRef.current) return;

    const scale = readAppliedInterfaceScaleFactor();
    const rect = anchorRef.current.getBoundingClientRect();
    const boundary = anchorRef.current.closest<HTMLElement>("[data-floating-boundary]");
    if (boundary) {
      const boundaryRect = boundary.getBoundingClientRect();
      if (rect.bottom <= boundaryRect.top || rect.top >= boundaryRect.bottom) {
        onCloseRef.current();
        return;
      }
    }
    const viewportWidth = interfaceLayoutPixels(window.innerWidth, scale);
    const viewportHeight = interfaceLayoutPixels(window.innerHeight, scale);
    const width = Math.min(
      Math.max(interfaceLayoutPixels(rect.width, scale), floatingWidth ?? 0),
      viewportWidth - FLOATING_MENU_MARGIN * 2,
    );
    const triggerLeft = interfaceLayoutPixels(rect.left, scale);
    const left = Math.min(
      Math.max(FLOATING_MENU_MARGIN, triggerLeft),
      viewportWidth - FLOATING_MENU_MARGIN - width,
    );
    const menuHeight = menuRef.current
      ? Math.min(menuRef.current.scrollHeight, FLOATING_MENU_MAX_HEIGHT)
      : FLOATING_MENU_MAX_HEIGHT;
    const placement = placeLookupPopover({
      anchor: {
        top: interfaceLayoutPixels(rect.top, scale),
        bottom: interfaceLayoutPixels(rect.bottom, scale),
        centerX: triggerLeft + width / 2,
      },
      popoverHeight: menuHeight,
      viewportHeight,
      gap: FLOATING_MENU_GAP,
      margin: FLOATING_MENU_MARGIN,
    });

    setFloatingSide(placement.side);
    setFloatingPosition({
      bottom: "auto",
      left,
      maxHeight: placement.maxHeight,
      right: "auto",
      top: placement.top,
      width,
    });
  }, [anchorRef, floating, floatingWidth, menuRef, open]);

  useEffect(() => {
    if (!menuMounted) setFloatingPosition(null);
  }, [menuMounted]);

  useLayoutEffect(() => {
    if (!floating) {
      setFloatingPosition(null);
      return;
    }
    if (!open) return;

    updateFloatingPosition();
    window.addEventListener(INTERFACE_LAYOUT_CHANGE_EVENT, updateFloatingPosition);
    document.addEventListener("scroll", updateFloatingPosition, true);
    return () => {
      window.removeEventListener(INTERFACE_LAYOUT_CHANGE_EVENT, updateFloatingPosition);
      document.removeEventListener("scroll", updateFloatingPosition, true);
    };
  }, [floating, open, updateFloatingPosition]);

  const floatingMenuMounted = floatingPosition !== null;
  useLayoutEffect(() => {
    if (!floating || !open || !floatingMenuMounted || !menuRef.current) return;
    updateFloatingPosition();
  }, [floating, floatingMenuMounted, open, updateFloatingPosition]);

  return { menuRef, menuMounted, floatingMenuMounted, floatingPosition, floatingSide };
}
