import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { createPortal } from "react-dom";
import {
  AudioLines,
  BookOpenText,
  GraduationCap,
  Info,
  Layers3,
  ScanEye,
  KeyRound,
  Languages,
  Link,
  SlidersHorizontal,
  Volume2,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import { INTERFACE_LAYOUT_CHANGE_EVENT, interfaceLayoutPixels } from "../../app/interface-scale";
import type { SettingsCategory } from "../settings-types";
import { useSettingsTabReorder } from "../hooks/useSettingsTabReorder";

export function SettingsTabBar({
  activeCategory,
  visibleCategories,
  onChange,
}: {
  activeCategory: SettingsCategory;
  visibleCategories: SettingsCategory[];
  onChange: (category: SettingsCategory) => void;
}) {
  const { t } = useTranslation();
  const reorder = useSettingsTabReorder();
  const [tooltip, setTooltip] = useState<{ label: string; top: number; left: number; keyboard: boolean } | null>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);
  const hideTooltip = () => setTooltip(null);
  const showTooltip = (target: HTMLButtonElement, label: string, keyboard = false) => {
    if (reorder.editing) return;
    const rect = target.getBoundingClientRect();
    setTooltip({
      label,
      top: interfaceLayoutPixels(rect.bottom) + 13,
      left: interfaceLayoutPixels(rect.left + rect.width / 2),
      keyboard,
    });
  };

  useEffect(() => {
    if (reorder.editing) hideTooltip();
  }, [reorder.editing]);

  useLayoutEffect(() => {
    const element = tooltipRef.current;
    if (!tooltip || !element) return;
    const width = element.offsetWidth;
    const left = Math.max(12, Math.min(tooltip.left - width / 2, interfaceLayoutPixels(window.innerWidth) - width - 12));
    element.style.left = `${left}px`;
    element.style.setProperty("--tooltip-arrow-left", `${tooltip.left - left}px`);
  }, [tooltip]);

  useEffect(() => {
    document.addEventListener("scroll", hideTooltip, true);
    window.addEventListener("resize", hideTooltip);
    window.addEventListener(INTERFACE_LAYOUT_CHANGE_EVENT, hideTooltip);
    return () => {
      document.removeEventListener("scroll", hideTooltip, true);
      window.removeEventListener("resize", hideTooltip);
      window.removeEventListener(INTERFACE_LAYOUT_CHANGE_EVENT, hideTooltip);
    };
  }, []);

  const categories = [
    { id: "system", label: t("settings.categories.system"), icon: <SlidersHorizontal size={18} /> },
    { id: "audio", label: t("settings.categories.audio"), icon: <Volume2 size={18} /> },
    { id: "recognition", label: t("settings.categories.recognition"), icon: <AudioLines size={18} /> },
    { id: "translation", label: t("settings.categories.translation"), icon: <Languages size={18} /> },
    { id: "vr_overlay", label: t("settings.categories.vrOverlay"), icon: <Layers3 size={18} /> },
    { id: "vision", label: t("settings.categories.vision"), icon: <ScanEye size={18} /> },
    { id: "glossary", label: t("settings.categories.glossary"), icon: <BookOpenText size={18} /> },
    { id: "api", label: t("settings.categories.api"), icon: <KeyRound size={18} /> },
    { id: "learning", label: t("settings.categories.learning"), icon: <GraduationCap size={18} /> },
    { id: "connections", label: t("settings.categories.connections"), icon: <Link size={18} /> },
    { id: "about", label: t("settings.categories.about"), icon: <Info size={18} /> },
  ] satisfies Array<{ id: SettingsCategory; label: string; icon: ReactNode }>;
  const visibleTabs = reorder.order.flatMap(id => {
    const category = categories.find(category => category.id === id);
    return category && visibleCategories.includes(id) ? [category] : [];
  });

  return (
    <div className="settings-tabbar-container">
      <div className="settings-tabbar-wrap" ref={reorder.scrollRef}>
        <div className={`settings-tabbar${reorder.editing ? " settings-tabbar-reordering" : ""}`} ref={reorder.tabbarRef}
          role="tablist" aria-label={t("settings.categories.label")}>
          {visibleTabs.map((category, index) => {
            const active = activeCategory === category.id;
            return (
              <button
                key={category.id}
                id={`settings-tab-${category.id}`}
                className={`${active ? "active" : ""}${reorder.dragging === category.id ? " settings-tab-dragging" : ""}`}
                data-settings-category={category.id}
                type="button"
                role="tab"
                aria-selected={active}
                aria-controls={`settings-panel-${category.id}`}
                aria-label={category.label}
                tabIndex={active ? 0 : -1}
                onPointerDown={(event) => { hideTooltip(); reorder.onPointerDown(category.id, event); }}
                onContextMenu={(event) => event.preventDefault()}
                onDragStart={(event) => event.preventDefault()}
                onPointerEnter={(event) => {
                  if (event.pointerType !== "touch") showTooltip(event.currentTarget, category.label);
                }}
                onPointerLeave={hideTooltip}
                onFocus={(event) => {
                  if (event.currentTarget.matches(":focus-visible")) showTooltip(event.currentTarget, category.label, true);
                }}
                onBlur={hideTooltip}
                onKeyDown={(event) => {
                  if (reorder.editing || event.altKey) return;
                  let nextIndex: number;
                  switch (event.key) {
                    case "Escape": hideTooltip(); return;
                    case "ArrowRight": nextIndex = (index + 1) % visibleTabs.length; break;
                    case "ArrowLeft": nextIndex = (index - 1 + visibleTabs.length) % visibleTabs.length; break;
                    case "Home": nextIndex = 0; break;
                    case "End": nextIndex = visibleTabs.length - 1; break;
                    default: return;
                  }
                  event.preventDefault();
                  onChange(visibleTabs[nextIndex].id);
                  event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>("button")[nextIndex]?.focus();
                }}
                onClick={(event) => {
                  hideTooltip();
                  if (reorder.suppressClick()) { event.preventDefault(); return; }
                  onChange(category.id);
                }}
              >
                <span className="settings-tab-icon" aria-hidden="true">{category.icon}</span>
                <span className="settings-tab-label">{category.label}</span>
              </button>
            );
          })}
        </div>
      </div>
      {tooltip && createPortal(
        <div
          ref={tooltipRef}
          className={`settings-tab-tooltip${tooltip.keyboard ? " keyboard" : ""}`}
          role="tooltip"
          aria-hidden="true"
          style={{ top: tooltip.top, left: tooltip.left, maxWidth: Math.min(220, interfaceLayoutPixels(window.innerWidth) - 24) }}
        >
          {tooltip.label}
        </div>,
        document.body,
      )}
    </div>
  );
}
