import type { ReactNode, Ref } from "react";
import { useTranslation } from "react-i18next";
import { GraduationCap, MessageSquare, MessageSquarePlus, Mic, Shrink, SlidersHorizontal, Square } from "lucide-react";

import type { Page } from "../app-types";

export function BottomDock({ page, running, chatboxOpen, captureDisabled, chatboxDisabled, chatboxButtonRef, onPageChange, onCompact, onChatbox, onCapture }: {
  page: Page;
  running: boolean;
  captureDisabled: boolean;
  chatboxOpen: boolean;
  chatboxDisabled: boolean;
  chatboxButtonRef: Ref<HTMLButtonElement>;
  onPageChange: (page: Page) => void;
  onCompact: () => void;
  onChatbox: () => void;
  onCapture: () => void;
}) {
  const { t } = useTranslation();
  return (
    <nav className={`bottom-dock ${chatboxOpen ? "chatbox-open" : ""}`} aria-label={t("navigation.main")}>
      <DockButton label={t("navigation.live")} active={page === "live"} onClick={() => onPageChange("live")}><MessageSquare /></DockButton>
      <DockButton label={t("navigation.learning")} active={page === "learning"} onClick={() => onPageChange("learning")}><GraduationCap /></DockButton>
      <DockButton label={t("navigation.settings")} active={page === "settings"} onClick={() => onPageChange("settings")}><SlidersHorizontal /></DockButton>
      <i className="dock-divider" aria-hidden="true" />
      <DockButton label={t("navigation.compact")} tonal onClick={onCompact}><Shrink /></DockButton>
      <DockButton
        label={t("navigation.chatbox")}
        active={chatboxOpen}
        disabled={chatboxDisabled}
        buttonRef={chatboxButtonRef}
        expanded={chatboxOpen}
        onClick={onChatbox}
      ><MessageSquarePlus /></DockButton>
      <DockButton label={t(running ? "capture.stop" : "capture.start")} primary disabled={captureDisabled} onClick={onCapture}>{running ? <Square /> : <Mic />}</DockButton>
    </nav>
  );
}

function DockButton({ label, active = false, tonal = false, primary = false, disabled = false, expanded, buttonRef, onClick, children }: {
  label: string;
  active?: boolean;
  tonal?: boolean;
  primary?: boolean;
  disabled?: boolean;
  expanded?: boolean;
  buttonRef?: Ref<HTMLButtonElement>;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      ref={buttonRef}
      className={`dock-button ${active ? "active" : ""} ${tonal ? "tonal" : ""} ${primary ? "primary" : ""}`}
      aria-label={label}
      aria-current={active && expanded === undefined ? "page" : undefined}
      aria-expanded={expanded}
      data-tooltip={label}
      disabled={disabled}
      onClick={onClick}
    >{children}</button>
  );
}
