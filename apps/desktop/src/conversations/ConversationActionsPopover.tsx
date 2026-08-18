import { useEffect, useRef } from "react";
import type { CSSProperties, KeyboardEvent as ReactKeyboardEvent } from "react";
import { createPortal } from "react-dom";
import {
  MessageSquareText,
  Pencil,
  RotateCcw,
  Shapes,
  Trash2,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import {
  interfaceLayoutPixels,
  readAppliedInterfaceScaleFactor,
} from "../app/interface-scale";
import { useDismissibleLayer } from "../shared/hooks/use-dismissible-layer";
import { CONVERSATION_ICON_KEYS } from "./conversations";
import type { ConversationIcon, ConversationSummary } from "./conversations";
import { ConversationIconView } from "./ConversationSidebarItems";

export type ConversationActionsPosition = {
  top: number;
  left: number;
  side?: "above" | "below";
};

export function conversationActionsPosition(
  clientX: number,
  clientY: number,
): ConversationActionsPosition {
  const scale = readAppliedInterfaceScaleFactor();
  const x = interfaceLayoutPixels(clientX, scale);
  const y = interfaceLayoutPixels(clientY, scale);
  const viewportWidth = interfaceLayoutPixels(window.innerWidth, scale);
  const viewportHeight = interfaceLayoutPixels(window.innerHeight, scale);
  const width = 186;
  const expectedHeight = 260;
  const gap = 4;
  const side = viewportHeight - y >= expectedHeight ? "below" : "above";
  return {
    top: side === "below" ? y + gap : y - gap,
    left: Math.max(8, Math.min(x + gap, viewportWidth - width - 8)),
    side,
  };
}

function movePopoverFocus(event: ReactKeyboardEvent<HTMLDivElement>) {
  const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
  const currentIndex = buttons.indexOf(document.activeElement as HTMLButtonElement);
  let nextIndex = currentIndex;
  if (event.key === "ArrowDown" || event.key === "ArrowRight") nextIndex = (currentIndex + 1) % buttons.length;
  else if (event.key === "ArrowUp" || event.key === "ArrowLeft") nextIndex = (currentIndex - 1 + buttons.length) % buttons.length;
  else if (event.key === "Home") nextIndex = 0;
  else if (event.key === "End") nextIndex = buttons.length - 1;
  else return;
  event.preventDefault();
  buttons[nextIndex]?.focus();
}

export function ConversationActionsPopover({
  conversation,
  position,
  choosingIcon,
  onChooseIcon,
  onStartRename,
  onShowIconPicker,
  onResetCustomization,
  onDelete,
  onClose,
}: {
  conversation: ConversationSummary;
  position: ConversationActionsPosition;
  choosingIcon: boolean;
  onChooseIcon: (icon: ConversationIcon | null) => void;
  onStartRename: () => void;
  onShowIconPicker: () => void;
  onResetCustomization: () => void;
  onDelete: () => void;
  onClose: (restoreFocus?: boolean) => void;
}) {
  const { t } = useTranslation();
  const layerRef = useRef<HTMLDivElement>(null);
  useDismissibleLayer(true, layerRef, () => onClose(true));

  useEffect(() => {
    window.requestAnimationFrame(() => {
      layerRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
    });
  }, [choosingIcon]);

  return createPortal(
    <div
      ref={layerRef}
      className={`conversation-actions-popover ${position.side === "above" ? "above" : ""} ${choosingIcon ? "choosing-icon" : ""}`}
      role={choosingIcon ? "group" : "menu"}
      aria-label={choosingIcon ? t("conversations.chooseIcon") : t("conversations.manage", { title: conversation.title })}
      style={{
        top: position.top,
        left: position.left,
      } as CSSProperties}
      onKeyDown={movePopoverFocus}
    >
      {choosingIcon ? (
        <>
          <span className="conversation-icon-picker-label">{t("conversations.chooseIcon")}</span>
          <div className="conversation-icon-picker">
            <button
              className={conversation.icon === "message" ? "selected" : ""}
              type="button"
              aria-label={t("conversations.icons.default")}
              title={t("conversations.icons.default")}
              onClick={() => onChooseIcon(null)}
            >
              <MessageSquareText size={18} />
            </button>
            {CONVERSATION_ICON_KEYS.filter((icon) => icon !== "message").map((icon) => (
              <button
                key={icon}
                className={conversation.icon === icon ? "selected" : ""}
                type="button"
                aria-label={t(`conversations.icons.${icon}`)}
                title={t(`conversations.icons.${icon}`)}
                onClick={() => onChooseIcon(icon)}
              >
                <ConversationIconView icon={icon} size={18} />
              </button>
            ))}
          </div>
        </>
      ) : (
        <>
          <button type="button" role="menuitem" onClick={onStartRename}><Pencil size={16} />{t("conversations.rename")}</button>
          <button type="button" role="menuitem" onClick={onShowIconPicker}><Shapes size={16} />{t("conversations.changeIcon")}</button>
          {conversation.customized && (
            <button type="button" role="menuitem" onClick={onResetCustomization}><RotateCcw size={16} />{t("conversations.resetCustomization")}</button>
          )}
          <button
            className="conversation-action-delete"
            type="button"
            role="menuitem"
            onClick={() => {
              if (!window.confirm(t("conversations.deleteConfirm", {
                title: conversation.title,
              }))) return;
              onDelete();
            }}
          ><Trash2 size={16} />{t("conversations.delete")}</button>
        </>
      )}
    </div>,
    document.body,
  );
}
