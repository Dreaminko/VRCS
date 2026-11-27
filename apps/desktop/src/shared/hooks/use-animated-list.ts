import { useEffect, useLayoutEffect, useRef, useState } from "react";

const EXIT_MS = 115;
const MOTION_MS = 190;
const MOTION_EASING = "cubic-bezier(0.16, 1, 0.3, 1)";

export function prefersReducedMotion(): boolean {
  return typeof window !== "undefined"
    && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;
}

/** Animates container height changes, item FLIP reordering, and delayed item exits. */
export function useAnimatedList(keys: readonly string[], enabled = true) {
  const [removingKeys, setRemovingKeys] = useState<string[]>([]);
  const timersRef = useRef<Map<string, number>>(new Map());
  const containerRef = useRef<HTMLElement | null>(null);
  const prevHeightRef = useRef<number | null>(null);
  const heightAnimRef = useRef<Animation | null>(null);
  const itemRefs = useRef<Map<string, HTMLElement>>(new Map());
  const prevRectsRef = useRef<Map<string, DOMRect>>(new Map());

  const layoutKey = enabled ? keys.join("\0") : "";

  useEffect(() => () => {
    for (const timer of timersRef.current.values()) {
      window.clearTimeout(timer);
    }
    timersRef.current.clear();
    heightAnimRef.current?.cancel();
    heightAnimRef.current = null;
    if (containerRef.current) containerRef.current.style.overflow = "";
  }, []);

  useEffect(() => {
    setRemovingKeys((current) => current.filter((key) => keys.includes(key)));
  }, [layoutKey]);

  useEffect(() => {
    const container = containerRef.current;
    if (!enabled || !container || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      if (!heightAnimRef.current) {
        prevHeightRef.current = container.getBoundingClientRect().height;
      }
    });
    observer.observe(container);
    return () => observer.disconnect();
  }, [enabled]);

  useLayoutEffect(() => {
    if (!enabled) {
      prevRectsRef.current.clear();
      prevHeightRef.current = null;
      heightAnimRef.current?.cancel();
      heightAnimRef.current = null;
      if (containerRef.current) containerRef.current.style.overflow = "";
      return;
    }

    const container = containerRef.current;
    if (container) {
      const activeHeight = heightAnimRef.current
        ? container.getBoundingClientRect().height
        : null;
      if (heightAnimRef.current) {
        heightAnimRef.current.cancel();
        heightAnimRef.current = null;
        container.style.overflow = "";
      }
      const nextHeight = container.getBoundingClientRect().height;
      const prevHeight = activeHeight ?? prevHeightRef.current;
      if (!prefersReducedMotion() && prevHeight !== null && Math.abs(prevHeight - nextHeight) >= 1) {
        container.style.overflow = "hidden";
        const anim = container.animate?.(
          [{ height: `${prevHeight}px` }, { height: `${nextHeight}px` }],
          { duration: MOTION_MS, easing: MOTION_EASING },
        );
        if (anim) {
          heightAnimRef.current = anim;
          const finish = () => {
            if (heightAnimRef.current === anim) {
              heightAnimRef.current = null;
              container.style.overflow = "";
            }
          };
          anim.onfinish = finish;
          anim.oncancel = finish;
        } else {
          container.style.overflow = "";
        }
      }
      prevHeightRef.current = nextHeight;
    }

    const nextRects = new Map<string, DOMRect>();
    for (const [key, element] of itemRefs.current) {
      nextRects.set(key, element.getBoundingClientRect());
    }
    if (!prefersReducedMotion() && prevRectsRef.current.size > 0) {
      for (const [key, nextRect] of nextRects) {
        const prevRect = prevRectsRef.current.get(key);
        const element = itemRefs.current.get(key);
        if (!prevRect || !element) continue;
        const dx = prevRect.left - nextRect.left;
        const dy = prevRect.top - nextRect.top;
        if (Math.abs(dx) < 1 && Math.abs(dy) < 1) continue;
        element.animate?.(
          [
            { transform: `translate(${dx}px, ${dy}px)` },
            { transform: "translate(0, 0)" },
          ],
          { duration: MOTION_MS, easing: MOTION_EASING },
        );
      }
    }
    prevRectsRef.current = nextRects;
  }, [layoutKey, enabled]);

  const bindContainerRef = (element: HTMLElement | null) => {
    containerRef.current = element;
  };

  const bindItemRef = (key: string) => (element: HTMLElement | null) => {
    if (element) itemRefs.current.set(key, element);
    else itemRefs.current.delete(key);
  };

  const isRemoving = (key: string) => removingKeys.includes(key);

  const scheduleRemove = (key: string, commit: () => void) => {
    if (prefersReducedMotion()) {
      commit();
      return;
    }
    if (timersRef.current.has(key)) return;
    setRemovingKeys((current) => (current.includes(key) ? current : [...current, key]));
    const timer = window.setTimeout(() => {
      timersRef.current.delete(key);
      setRemovingKeys((current) => current.filter((item) => item !== key));
      commit();
    }, EXIT_MS);
    timersRef.current.set(key, timer);
  };

  const clearPendingRemovals = () => {
    for (const timer of timersRef.current.values()) {
      window.clearTimeout(timer);
    }
    timersRef.current.clear();
    setRemovingKeys([]);
  };

  return {
    bindContainerRef,
    bindItemRef,
    isRemoving,
    scheduleRemove,
    clearPendingRemovals,
  };
}
