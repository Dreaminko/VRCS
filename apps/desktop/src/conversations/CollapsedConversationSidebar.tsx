import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { PanelLeftOpen, Plus, Search } from "lucide-react";
import { useTranslation } from "react-i18next";

import {
  interfaceLayoutPixels,
  readAppliedInterfaceScaleFactor,
} from "../app/interface-scale";
import type { ConversationSummary } from "./conversations";
import { RailConversationButton } from "./ConversationSidebarItems";

type SidebarTooltip = {
  top: number;
  left: number;
  maxWidth: number;
  title: string;
  detail?: string;
  current?: boolean;
  keyboard: boolean;
  visible: boolean;
};

const TOOLTIP_HOVER_DELAY_MS = 180;
const TOOLTIP_SWITCH_DELAY_MS = 70;
const TOOLTIP_EXIT_MS = 110;

export function CollapsedConversationSidebar({
  conversations,
  activeId,
  selectedId,
  onToggle,
  onNew,
  onSelect,
  onSearch,
}: {
  conversations: ConversationSummary[];
  activeId?: string;
  selectedId?: string;
  onToggle: () => void;
  onNew: () => void;
  onSelect: (id: string) => void;
  onSearch: () => void;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const [tooltip, setTooltip] = useState<SidebarTooltip | null>(null);
  const tooltipTimerRef = useRef<number | null>(null);
  const tooltipHideTimerRef = useRef<number | null>(null);
  const tooltipFrameRef = useRef<number | null>(null);

  const clearTooltipTimer = () => {
    if (tooltipTimerRef.current === null) return;
    window.clearTimeout(tooltipTimerRef.current);
    tooltipTimerRef.current = null;
  };

  const clearTooltipHideTimer = () => {
    if (tooltipHideTimerRef.current === null) return;
    window.clearTimeout(tooltipHideTimerRef.current);
    tooltipHideTimerRef.current = null;
  };

  const clearTooltipFrame = () => {
    if (tooltipFrameRef.current === null) return;
    window.cancelAnimationFrame(tooltipFrameRef.current);
    tooltipFrameRef.current = null;
  };

  const hideTooltip = () => {
    clearTooltipTimer();
    clearTooltipFrame();
    clearTooltipHideTimer();
    setTooltip((current) => current ? { ...current, visible: false } : null);
    tooltipHideTimerRef.current = window.setTimeout(() => {
      setTooltip(null);
      tooltipHideTimerRef.current = null;
    }, TOOLTIP_EXIT_MS);
  };

  const showTooltip = (
    target: HTMLButtonElement,
    title: string,
    detail?: string,
    current = false,
    keyboard = false,
  ) => {
    clearTooltipTimer();
    clearTooltipHideTimer();
    clearTooltipFrame();
    const reveal = () => {
      const rect = target.getBoundingClientRect();
      const scale = readAppliedInterfaceScaleFactor();
      const viewportWidth = interfaceLayoutPixels(window.innerWidth, scale);
      const viewportHeight = interfaceLayoutPixels(window.innerHeight, scale);
      const left = interfaceLayoutPixels(rect.right, scale) + 10;
      setTooltip({
        top: Math.max(30, Math.min(
          interfaceLayoutPixels(rect.top + rect.height / 2, scale),
          viewportHeight - 30,
        )),
        left,
        maxWidth: Math.max(140, Math.min(240, viewportWidth - left - 12)),
        title,
        detail,
        current,
        keyboard,
        visible: keyboard,
      });
      if (!keyboard) {
        tooltipFrameRef.current = window.requestAnimationFrame(() => {
          setTooltip((currentTooltip) => currentTooltip
            ? { ...currentTooltip, visible: true }
            : null);
          tooltipFrameRef.current = null;
        });
      }
      tooltipTimerRef.current = null;
    };
    if (keyboard) reveal();
    else {
      tooltipTimerRef.current = window.setTimeout(
        reveal,
        tooltip ? TOOLTIP_SWITCH_DELAY_MS : TOOLTIP_HOVER_DELAY_MS,
      );
    }
  };

  useEffect(() => () => {
    clearTooltipTimer();
    clearTooltipHideTimer();
    clearTooltipFrame();
  }, []);

  return (
    <>
      <div className="conversation-sidebar-collapsed-content">
        <button
          className="sidebar-icon-button"
          type="button"
          aria-label={t("conversations.expandSidebar")}
          aria-expanded="false"
          onPointerEnter={(event) => showTooltip(event.currentTarget, t("conversations.expandSidebar"))}
          onPointerLeave={hideTooltip}
          onFocus={(event) => showTooltip(event.currentTarget, t("conversations.expandSidebar"), undefined, false, event.currentTarget.matches(":focus-visible"))}
          onBlur={hideTooltip}
          onClick={onToggle}
        ><PanelLeftOpen size={19} /></button>
        <button
          className="sidebar-icon-button sidebar-new-icon"
          type="button"
          aria-label={t("conversations.create")}
          onPointerEnter={(event) => showTooltip(event.currentTarget, t("conversations.create"))}
          onPointerLeave={hideTooltip}
          onFocus={(event) => showTooltip(event.currentTarget, t("conversations.create"), undefined, false, event.currentTarget.matches(":focus-visible"))}
          onBlur={hideTooltip}
          onClick={() => {
            hideTooltip();
            onNew();
          }}
        ><Plus size={20} /></button>
        <button
          className="sidebar-icon-button sidebar-search-icon"
          type="button"
          aria-label={t("conversations.search")}
          onPointerEnter={(event) => showTooltip(event.currentTarget, t("conversations.search"))}
          onPointerLeave={hideTooltip}
          onFocus={(event) => showTooltip(event.currentTarget, t("conversations.search"), undefined, false, event.currentTarget.matches(":focus-visible"))}
          onBlur={hideTooltip}
          onClick={() => {
            hideTooltip();
            onSearch();
          }}
        ><Search size={18} /></button>
        <div className="sidebar-rail-divider" />
        <div className="sidebar-conversation-rail" aria-label={t("conversations.recent")} onScroll={hideTooltip}>
          {conversations.map((conversation) => (
            <RailConversationButton
              key={conversation.id}
              conversation={conversation}
              active={conversation.id === activeId}
              selected={conversation.id === selectedId}
              locale={locale}
              onSelect={(id) => {
                hideTooltip();
                onSelect(id);
              }}
              onShowTooltip={showTooltip}
              onHideTooltip={hideTooltip}
            />
          ))}
        </div>
      </div>
      {tooltip && createPortal(
        <div
          className={`sidebar-rail-tooltip ${tooltip.visible ? "visible" : ""} ${tooltip.keyboard ? "keyboard" : ""}`}
          role="tooltip"
          style={{
            top: tooltip.top,
            left: tooltip.left,
            maxWidth: tooltip.maxWidth,
          }}
        >
          <span className="sidebar-rail-tooltip-title">
            {tooltip.current && <i aria-hidden="true" />}
            <strong>{tooltip.title}</strong>
          </span>
          {tooltip.detail && <span className="sidebar-rail-tooltip-detail">{tooltip.detail}</span>}
        </div>,
        document.body,
      )}
    </>
  );
}
