import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";

import { prefersReducedMotion } from "../../app/app-motion";
import { INTERFACE_LAYOUT_CHANGE_EVENT, interfaceLayoutPixels } from "../../app/interface-scale";
import { moveSettingsTab, readSettingsTabOrder, saveSettingsTabOrder } from "../settings-tab-order";
import type { SettingsCategory } from "../settings-types";

const HOLD_DELAY = 500;
const MOVE_TOLERANCE = 8;
const REORDER_DURATION = 240;
const REORDER_EASING = "cubic-bezier(0.16, 1, 0.3, 1)";

type Press = {
  category: SettingsCategory;
  target: HTMLButtonElement;
  pointerId: number;
  pointerType: string;
  startX: number;
  startY: number;
  clientX: number;
  startScrollLeft: number;
  grabOffset: number;
  started: boolean;
  scrolling: boolean;
  initialOrder: SettingsCategory[];
};

export function useSettingsTabReorder() {
  const [order, setOrder] = useState(readSettingsTabOrder);
  const [editing, setEditing] = useState(false);
  const [dragging, setDragging] = useState<SettingsCategory | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const tabbarRef = useRef<HTMLDivElement>(null);
  const orderRef = useRef(order);
  const editingRef = useRef(false);
  const pressRef = useRef<Press | null>(null);
  const timerRef = useRef<number | undefined>(undefined);
  const frameRef = useRef<number | undefined>(undefined);
  const suppressClickRef = useRef(false);
  const positionsRef = useRef<Map<HTMLButtonElement, number> | null>(null);
  const animationsRef = useRef(new Map<HTMLButtonElement, Animation>());

  function tabPositions() {
    return new Map([...tabbarRef.current?.querySelectorAll<HTMLButtonElement>("[role=tab]") ?? []]
      .map(tab => [tab, tab.getBoundingClientRect().left] as const));
  }

  function cancelAnimation(tab: HTMLButtonElement) {
    animationsRef.current.get(tab)?.cancel();
    animationsRef.current.delete(tab);
  }

  function animatePositions(previous: Map<HTMLButtonElement, number>) {
    const bar = tabbarRef.current;
    if (!bar) return;
    const tabs = [...bar.querySelectorAll<HTMLButtonElement>("[role=tab]")];
    for (const tab of tabs) cancelAnimation(tab);
    if (prefersReducedMotion()) return;
    const barLeft = bar.getBoundingClientRect().left;
    for (const tab of tabs) {
      const from = previous.get(tab);
      if (from === undefined || tab === pressRef.current?.target || !tab.animate) continue;
      const offset = interfaceLayoutPixels(from - barLeft) - bar.clientLeft - tab.offsetLeft;
      if (Math.abs(offset) < 0.5) continue;
      const animation = tab.animate([
        { transform: `translateX(${offset}px)` },
        { transform: "translateX(0px)" },
      ], { duration: REORDER_DURATION, easing: REORDER_EASING, fill: "both" });
      animationsRef.current.set(tab, animation);
      void animation.finished.then(() => {
        if (animationsRef.current.get(tab) === animation) cancelAnimation(tab);
      }, () => {});
    }
  }

  function updateOrder(next: SettingsCategory[]) {
    if (next === orderRef.current) return;
    positionsRef.current ??= tabPositions();
    orderRef.current = next;
    setOrder(next);
  }

  function clearTimer() {
    window.clearTimeout(timerRef.current);
    timerRef.current = undefined;
  }

  function releasePointer(press: Press) {
    if (press.target.hasPointerCapture(press.pointerId)) press.target.releasePointerCapture(press.pointerId);
    press.target.style.removeProperty("--settings-tab-drag-x");
  }

  function capturePointer(press: Press) {
    try {
      press.target.setPointerCapture(press.pointerId);
    } catch {
      // Window listeners also handle pointers without capture support.
    }
  }

  function finishDrag(cancelled: boolean) {
    clearTimer();
    if (frameRef.current !== undefined) cancelAnimationFrame(frameRef.current);
    frameRef.current = undefined;
    const press = pressRef.current;
    if (press?.started) positionsRef.current ??= tabPositions();
    pressRef.current = null;
    if (!press) return;
    releasePointer(press);
    setDragging(null);
    if (!press.started) return;
    if (cancelled) updateOrder(press.initialOrder);
    else if (orderRef.current !== press.initialOrder) saveSettingsTabOrder(orderRef.current);
  }

  function exitEditing() {
    finishDrag(true);
    editingRef.current = false;
    setEditing(false);
  }

  function positionDragged(press: Press) {
    const bar = tabbarRef.current;
    if (!bar) return null;
    const bounds = bar.getBoundingClientRect();
    const scale = 1 / interfaceLayoutPixels(1);
    const width = press.target.offsetWidth * scale;
    const left = Math.max(bounds.left, Math.min(press.clientX - press.grabOffset, bounds.right - width));
    const offset = interfaceLayoutPixels(left - bounds.left) - bar.clientLeft - press.target.offsetLeft;
    press.target.style.setProperty("--settings-tab-drag-x", `${offset}px`);
    return left + width / 2;
  }

  function updateDrag() {
    const press = pressRef.current;
    const scroller = scrollRef.current;
    const bar = tabbarRef.current;
    if (!press?.started || !scroller || !bar) return;
    if (!press.target.isConnected) { exitEditing(); return; }
    const bounds = scroller.getBoundingClientRect();
    const edge = 32;
    const speed = press.clientX < bounds.left + edge
      ? -Math.min(12, (bounds.left + edge - press.clientX) / 2)
      : press.clientX > bounds.right - edge
        ? Math.min(12, (press.clientX - bounds.right + edge) / 2) : 0;
    scroller.scrollLeft += interfaceLayoutPixels(speed);
    const center = positionDragged(press);
    if (center !== null) {
      const tabs = [...bar.querySelectorAll<HTMLButtonElement>("[role=tab]")];
      const from = tabs.indexOf(press.target);
      const layoutCenter = interfaceLayoutPixels(center - bar.getBoundingClientRect().left);
      // Use layout positions so animated neighbors do not move the drop target.
      const to = tabs.filter(tab => {
        if (tab === press.target) return false;
        return bar.clientLeft + tab.offsetLeft + tab.offsetWidth / 2 < layoutCenter;
      }).length;
      const target = tabs[to]?.dataset.settingsCategory as SettingsCategory | undefined;
      if (from !== to && target) updateOrder(moveSettingsTab(orderRef.current, press.category, target));
    }
  }

  function animateDrag() {
    updateDrag();
    if (pressRef.current?.started) frameRef.current = requestAnimationFrame(animateDrag);
  }

  function startDrag(press: Press) {
    if (pressRef.current !== press || !press.target.isConnected) return;
    clearTimer();
    cancelAnimation(press.target);
    press.started = true;
    suppressClickRef.current = true;
    editingRef.current = true;
    setEditing(true);
    setDragging(press.category);
    frameRef.current = requestAnimationFrame(animateDrag);
  }

  function onPointerDown(category: SettingsCategory, event: ReactPointerEvent<HTMLButtonElement>) {
    if (!event.isPrimary || event.button !== 0 || pressRef.current) return;
    suppressClickRef.current = false;
    const rect = event.currentTarget.getBoundingClientRect();
    const press: Press = {
      category, target: event.currentTarget, pointerId: event.pointerId, pointerType: event.pointerType,
      startX: event.clientX, startY: event.clientY, clientX: event.clientX,
      startScrollLeft: scrollRef.current?.scrollLeft ?? 0,
      grabOffset: event.clientX - rect.left, started: false, scrolling: false,
      initialOrder: orderRef.current,
    };
    pressRef.current = press;
    capturePointer(press);
    if (editingRef.current) startDrag(press);
    else timerRef.current = window.setTimeout(() => startDrag(press), HOLD_DELAY);
  }

  function suppressClick(): boolean {
    const suppressed = suppressClickRef.current || editingRef.current;
    suppressClickRef.current = false;
    return suppressed;
  }

  useLayoutEffect(() => {
    const previous = positionsRef.current;
    positionsRef.current = null;
    if (previous) animatePositions(previous);
    const press = pressRef.current;
    if (press?.started) {
      // Moving a keyed DOM node can release its pointer capture.
      capturePointer(press);
      positionDragged(press);
    }
  }, [order, dragging]);

  useEffect(() => {
    const move = (event: PointerEvent) => {
      const press = pressRef.current;
      if (!press || press.pointerId !== event.pointerId) return;
      press.clientX = event.clientX;
      if (!press.started) {
        if (Math.hypot(event.clientX - press.startX, event.clientY - press.startY) > MOVE_TOLERANCE) {
          clearTimer();
          press.scrolling = true;
          suppressClickRef.current = true;
        }
        if (press.scrolling && press.pointerType === "touch" && scrollRef.current) {
          scrollRef.current.scrollLeft = press.startScrollLeft - interfaceLayoutPixels(event.clientX - press.startX);
        }
      }
      if (press.started && event.cancelable) event.preventDefault();
    };
    const up = (event: PointerEvent) => {
      const press = pressRef.current;
      if (press?.pointerId === event.pointerId) {
        press.clientX = event.clientX;
        if (press.started) updateDrag();
        finishDrag(false);
      }
    };
    const cancel = (event: PointerEvent) => {
      if (pressRef.current?.pointerId === event.pointerId) finishDrag(true);
    };
    const lostCapture = (event: PointerEvent) => {
      const press = pressRef.current;
      if (press?.pointerId === event.pointerId && !press.target.hasPointerCapture(event.pointerId)) finishDrag(true);
    };
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !tabbarRef.current?.contains(event.target)) exitEditing();
    };
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && (editingRef.current || pressRef.current)) {
        event.preventDefault();
        exitEditing();
      }
    };
    window.addEventListener("pointermove", move, { passive: false });
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("lostpointercapture", lostCapture);
    window.addEventListener("blur", exitEditing);
    window.addEventListener("resize", exitEditing);
    window.addEventListener(INTERFACE_LAYOUT_CHANGE_EVENT, exitEditing);
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("keydown", keydown, true);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("lostpointercapture", lostCapture);
      window.removeEventListener("blur", exitEditing);
      window.removeEventListener("resize", exitEditing);
      window.removeEventListener(INTERFACE_LAYOUT_CHANGE_EVENT, exitEditing);
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("keydown", keydown, true);
      clearTimer();
      if (frameRef.current !== undefined) cancelAnimationFrame(frameRef.current);
      const press = pressRef.current;
      pressRef.current = null;
      if (press) releasePointer(press);
      for (const tab of animationsRef.current.keys()) cancelAnimation(tab);
      positionsRef.current = null;
    };
  }, []);

  return { order, editing, dragging, scrollRef, tabbarRef, onPointerDown, suppressClick };
}
