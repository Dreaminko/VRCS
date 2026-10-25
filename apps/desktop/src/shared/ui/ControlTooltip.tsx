import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { INTERFACE_LAYOUT_CHANGE_EVENT, interfaceLayoutPixels } from "../../app/interface-scale";

type Tooltip = { target: HTMLElement; label: string; keyboard: boolean };

// Dock buttons already paint their own tooltips with CSS.
const SELECTOR = "[data-tooltip]:not(.dock-button)";

export function ControlTooltip() {
  const [tooltip, setTooltip] = useState<Tooltip | null>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);
  const id = useId();

  useEffect(() => {
    let target: HTMLElement | null = null;
    let timer: number | undefined;
    let observer: MutationObserver | undefined;
    const hide = () => {
      window.clearTimeout(timer);
      observer?.disconnect();
      target = null;
      setTooltip(null);
    };
    const labelFor = (element: HTMLElement) => element.isConnected
      && !element.matches('[aria-haspopup]:not([aria-haspopup="false"])[aria-expanded="true"]')
      ? element.dataset.tooltip?.trim() : undefined;
    const show = (element: HTMLElement | null, keyboard = false) => {
      if (element === target) return;
      hide();
      if (!element || !labelFor(element)) return;
      target = element;
      const reveal = () => {
        const label = labelFor(element);
        if (label) setTooltip({ target: element, label, keyboard });
        else hide();
      };
      if (keyboard) reveal();
      else timer = window.setTimeout(reveal, 180);
      observer = new MutationObserver(() => {
        const label = labelFor(element);
        if (!label) hide();
        else setTooltip(current => current?.target === element && current.label !== label
          ? { ...current, label } : current);
      });
      observer.observe(document.body, {
        subtree: true, childList: true, attributes: true,
        attributeFilter: ["data-tooltip", "aria-haspopup", "aria-expanded"],
      });
    };
    const closest = (node: EventTarget | null) => node instanceof Element
      ? node.closest<HTMLElement>(SELECTOR) : null;
    const over = (event: PointerEvent) => {
      if (event.pointerType !== "touch") show(closest(event.target));
    };
    const out = (event: PointerEvent) => {
      if (target && !(event.relatedTarget instanceof Node && target.contains(event.relatedTarget))) hide();
    };
    const focus = (event: FocusEvent) => {
      const element = closest(event.target);
      if (element?.matches(":focus-visible")) show(element, true);
    };
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") hide(); };
    document.addEventListener("pointerover", over);
    document.addEventListener("pointerout", out);
    document.addEventListener("focusin", focus);
    document.addEventListener("focusout", hide);
    document.addEventListener("pointerdown", hide, true);
    document.addEventListener("click", hide, true);
    document.addEventListener("keydown", key, true);
    document.addEventListener("scroll", hide, true);
    document.addEventListener("visibilitychange", hide);
    window.addEventListener("blur", hide);
    window.addEventListener("resize", hide);
    window.addEventListener(INTERFACE_LAYOUT_CHANGE_EVENT, hide);
    return () => {
      window.clearTimeout(timer);
      observer?.disconnect();
      document.removeEventListener("pointerover", over);
      document.removeEventListener("pointerout", out);
      document.removeEventListener("focusin", focus);
      document.removeEventListener("focusout", hide);
      document.removeEventListener("pointerdown", hide, true);
      document.removeEventListener("click", hide, true);
      document.removeEventListener("keydown", key, true);
      document.removeEventListener("scroll", hide, true);
      document.removeEventListener("visibilitychange", hide);
      window.removeEventListener("blur", hide);
      window.removeEventListener("resize", hide);
      window.removeEventListener(INTERFACE_LAYOUT_CHANGE_EVENT, hide);
    };
  }, []);

  useLayoutEffect(() => {
    const element = tooltipRef.current;
    if (!tooltip || !element) return;
    const { target } = tooltip;
    const position = () => {
      const rect = target.getBoundingClientRect();
      const gutter = 8;
      const gap = 10;
      const viewportWidth = interfaceLayoutPixels(window.innerWidth);
      const viewportHeight = interfaceLayoutPixels(window.innerHeight);
      element.style.maxWidth = `${Math.max(0, Math.min(220, viewportWidth - gutter * 2))}px`;
      element.style.setProperty("--tooltip-max-height", `${Math.max(0, viewportHeight - gutter * 2)}px`);
      const width = element.offsetWidth;
      const height = element.offsetHeight;
      const center = interfaceLayoutPixels(rect.left + rect.width / 2);
      const above = interfaceLayoutPixels(rect.top) - gap - height;
      const below = interfaceLayoutPixels(rect.bottom) + gap;
      const side = below + height <= viewportHeight - gutter || above < gutter ? "bottom" : "top";
      const left = Math.max(gutter, Math.min(center - width / 2, viewportWidth - width - gutter));
      const top = Math.max(gutter, Math.min(side === "bottom" ? below : above, viewportHeight - height - gutter));
      element.style.left = `${left}px`;
      element.style.top = `${top}px`;
      element.style.setProperty("--tooltip-arrow-left", `${Math.max(12, Math.min(center - left, width - 12))}px`);
      element.dataset.side = side;
    };
    position();
    const observer = new ResizeObserver(position);
    observer.observe(element);
    observer.observe(target);

    const description = target.getAttribute("aria-describedby")?.split(/\s+/).filter(Boolean) ?? [];
    target.setAttribute("aria-describedby", [...description, id].join(" "));
    return () => {
      observer.disconnect();
      const remaining = target.getAttribute("aria-describedby")?.split(/\s+/).filter(value => value && value !== id);
      if (remaining?.length) target.setAttribute("aria-describedby", remaining.join(" "));
      else target.removeAttribute("aria-describedby");
    };
  }, [tooltip, id]);

  return tooltip && createPortal(
    <div ref={tooltipRef} id={id} role="tooltip" className={`control-tooltip${tooltip.keyboard ? " keyboard" : ""}`}>
      <span className="control-tooltip-label">{tooltip.label}</span>
    </div>,
    document.body,
  );
}
