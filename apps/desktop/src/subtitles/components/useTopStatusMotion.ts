import { useLayoutEffect, useRef } from "react";
import { prefersReducedMotion } from "../../app/app-motion";

const STATUS_MOTION = {
  expand: { duration: 260, easing: "cubic-bezier(0.16, 1, 0.3, 1)" },
  collapse: { duration: 200, easing: "cubic-bezier(0.4, 0, 0.2, 1)" },
};

type Shape = { width: number; height: number; radius: number };

function surfaceShape(element: HTMLElement): Shape {
  const style = getComputedStyle(element);
  const width = parseFloat(style.width), height = parseFloat(style.height);
  const radius = parseFloat(style.borderTopLeftRadius) * (style.borderTopLeftRadius.endsWith("%") ? width / 100 : 1);
  return { width, height, radius: Math.min(radius, width / 2, height / 2) };
}

function clipShape(shape: Shape, canvas: Shape, inset = 0) {
  return `inset(${inset}px ${inset}px ${canvas.height - shape.height + inset}px ${canvas.width - shape.width + inset}px round ${Math.max(0, shape.radius - inset)}px)`;
}

export function useTopStatusMotion(expanded: boolean) {
  const rowRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const surfaceRef = useRef<HTMLSpanElement>(null);
  const outlineRef = useRef<HTMLSpanElement>(null);
  const fillRef = useRef<HTMLSpanElement>(null);
  const contentRef = useRef<HTMLSpanElement>(null);
  const animationsRef = useRef<Animation[]>([]);
  const frameRef = useRef<{ from: Shape; to: Shape; animation: Animation } | null>(null);
  const previousRef = useRef<{ surface: DOMRect; radius: number; controls?: DOMRect } | null>(null);

  const cancel = () => {
    for (const animation of animationsRef.current) animation.cancel();
    animationsRef.current = [];
    frameRef.current = null;
    surfaceRef.current?.style.removeProperty("width");
    surfaceRef.current?.style.removeProperty("height");
  };
  const prepare = (pointer: boolean) => {
    const surface = surfaceRef.current;
    if (!pointer || prefersReducedMotion() || !surface?.animate) {
      previousRef.current = null;
      cancel();
      return;
    }
    const bounds = surface.getBoundingClientRect();
    let shape = surfaceShape(surface);
    const zoom = bounds.width / shape.width;
    const frame = frameRef.current;
    if (frame) {
      const progress = frame.animation.effect?.getComputedTiming().progress ?? 1;
      const interpolate = (key: keyof Shape) => frame.from[key] + (frame.to[key] - frame.from[key]) * progress;
      shape = { width: interpolate("width"), height: interpolate("height"), radius: interpolate("radius") };
    }
    const controls = rowRef.current?.firstElementChild;
    previousRef.current = {
      surface: new DOMRect(bounds.right - shape.width * zoom, bounds.top, shape.width * zoom, shape.height * zoom),
      radius: shape.radius * zoom,
      controls: controls !== buttonRef.current ? controls?.getBoundingClientRect() : undefined,
    };
  };

  useLayoutEffect(() => {
    const previous = previousRef.current;
    previousRef.current = null;
    cancel();
    const row = rowRef.current, button = buttonRef.current;
    const surface = surfaceRef.current, content = contentRef.current;
    const outline = outlineRef.current, fill = fillRef.current;
    if (!previous || !row || !button || !surface || !content || !outline || !fill || prefersReducedMotion()) return;

    const bounds = surface.getBoundingClientRect();
    const to = surfaceShape(surface);
    const zoom = bounds.width / to.width;
    const from = { width: previous.surface.width / zoom, height: previous.surface.height / zoom, radius: previous.radius / zoom };
    const x = (previous.surface.right - bounds.right) / zoom;
    const y = (previous.surface.top - bounds.top) / zoom;
    const canvas = { ...to, width: Math.max(from.width, to.width), height: Math.max(from.height, to.height) };
    surface.style.width = `${canvas.width}px`;
    surface.style.height = `${canvas.height}px`;
    const options = { ...STATUS_MOTION[expanded ? "expand" : "collapse"], fill: "both" as const };

    // Clip a fixed-size surface instead of scaling it, so corners and shadows stay round.
    const outlineAnimation = outline.animate([
      { clipPath: clipShape(from, canvas) }, { clipPath: clipShape(to, canvas) },
    ], options);
    frameRef.current = { from, to, animation: outlineAnimation };
    const animations = [outlineAnimation, fill.animate([
      { clipPath: clipShape(from, canvas, 1) }, { clipPath: clipShape(to, canvas, 1) },
    ], options), surface.animate([
      { transform: `translate(${x}px, ${y}px)` }, { transform: "none" },
    ], options), content.animate([
      { opacity: 0, transform: `translateX(${expanded ? 6 : -4}px)` },
      { opacity: 1, transform: "none" },
    ], options)];

    // Move the row, then counter-move the status button to keep its right edge fixed.
    // The personnel panel keeps the same containing block, and text is never scaled.
    if (previous.controls && row.firstElementChild) {
      const controls = row.firstElementChild.getBoundingClientRect();
      const dx = (previous.controls.left - controls.left) / zoom;
      const dy = (previous.controls.top - controls.top) / zoom;
      animations.push(
        row.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], options),
        button.animate([{ transform: `translate(${-dx}px, ${-dy}px)` }, { transform: "none" }], options),
      );
    }
    animationsRef.current = animations;
    void Promise.all(animations.map(animation => animation.finished)).then(() => {
      if (animationsRef.current === animations) cancel();
    }, () => {});
  }, [expanded]);

  useLayoutEffect(() => cancel, []);
  return { rowRef, buttonRef, surfaceRef, outlineRef, fillRef, contentRef, prepare };
}
