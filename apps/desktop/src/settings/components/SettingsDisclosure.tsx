import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { prefersReducedMotion } from "../../app/app-motion";

export function SettingsDisclosure({ open, children, className = "" }: {
  open: boolean;
  children: ReactNode;
  className?: string;
}) {
  const [retained, setRetained] = useState(open);
  const contentRef = useRef<HTMLDivElement | null>(null);
  const animationRef = useRef<Animation | null>(null);
  const previousOpenRef = useRef(open);
  const mounted = open || retained;

  useLayoutEffect(() => {
    const changed = previousOpenRef.current !== open;
    previousOpenRef.current = open;
    const content = contentRef.current;
    const previous = animationRef.current;
    const style = content && previous ? getComputedStyle(content) : null;
    const current = style ? {
      height: style.height, opacity: style.opacity, marginBlockStart: style.marginBlockStart,
    } : null;
    previous?.cancel();
    animationRef.current = null;
    if (!content) return;

    setRetained(open);
    if (!changed || !content.animate || prefersReducedMotion()) {
      content.style.overflow = "";
      return;
    }

    setRetained(true);
    const parent = content.parentElement;
    const gap = parent && parent.children.length > 1 ? parseFloat(getComputedStyle(parent).rowGap) || 0 : 0;
    const collapsed = { height: "0px", opacity: 0, marginBlockStart: `${-gap}px` };
    const expanded = { height: getComputedStyle(content).height, opacity: 1, marginBlockStart: "0px" };
    // Include the parent's gap so removing the collapsed region does not shift its neighbors.
    content.style.overflow = "clip";
    const animation = content.animate([current ?? (open ? collapsed : expanded), open ? expanded : collapsed], {
      duration: open ? 220 : 180,
      easing: open ? "cubic-bezier(0.16, 1, 0.3, 1)" : "cubic-bezier(0.4, 0, 1, 1)",
      fill: "both",
    });
    animationRef.current = animation;
    void animation.finished.then(() => {
      if (animationRef.current !== animation) return;
      if (open) {
        animation.cancel();
        animationRef.current = null;
        content.style.overflow = "";
      } else {
        setRetained(false);
      }
    }, () => {});
  }, [mounted, open]);

  useLayoutEffect(() => () => {
    animationRef.current?.cancel();
    animationRef.current = null;
  }, []);

  return mounted ? <div ref={contentRef} className={`settings-disclosure ${className}`}
    aria-hidden={!open} inert={!open}>{children}</div> : null;
}
