import { useLayoutEffect, useRef, useState } from "react";
import { prefersReducedMotion } from "../../app/app-motion";

export function useDropdownMenuMotion(open: boolean, ready = true) {
  const [retained, setRetained] = useState(false);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const animationRef = useRef<Animation | null>(null);
  const mounted = open || retained;

  useLayoutEffect(() => {
    const menu = menuRef.current;
    const previous = animationRef.current;
    const style = menu && previous ? getComputedStyle(menu) : null;
    const current = style ? { opacity: style.opacity, transform: style.transform } : null;
    previous?.cancel();
    animationRef.current = null;
    if (!menu) return;

    setRetained(open);
    if (!menu.animate || prefersReducedMotion()) return;

    // Keep the surface mounted until its exit finishes, without delaying dismissal.
    setRetained(true);
    const collapsed = {
      opacity: 0,
      transform: `translateY(${menu.dataset.side === "above" ? 6 : -6}px) scale(0.97)`,
    };
    const expanded = { opacity: 1, transform: "translateY(0) scale(1)" };
    const animation = menu.animate([
      current ?? (open ? collapsed : expanded),
      open ? expanded : collapsed,
    ], {
      duration: open ? 180 : 120,
      easing: open ? "cubic-bezier(0.16, 1, 0.3, 1)" : "cubic-bezier(0.4, 0, 1, 1)",
      fill: "both",
    });
    animationRef.current = animation;
    void animation.finished.then(() => {
      if (animationRef.current !== animation) return;
      if (open) {
        animation.cancel();
        animationRef.current = null;
      } else {
        setRetained(false);
      }
    }, () => {});
  }, [mounted, open, ready]);

  useLayoutEffect(() => () => animationRef.current?.cancel(), []);

  return { menuRef, mounted };
}
