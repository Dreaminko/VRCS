import type { ReactNode, Ref } from "react";
import { useTranslation } from "react-i18next";
import { Eye, GraduationCap, MessageSquare, MessageSquarePlus, Mic, Radar, Shrink, SlidersHorizontal, Square } from "lucide-react";

import type { Page } from "../app/app-types";

export function BottomDock({
  learningAvailable,
  chatboxAvailable,
  page,
  running,
  chatboxOpen,
  captureDisabled,
  captureReason,
  networkCaptureVisible = false,
  networkCaptureRunning = false,
  networkCaptureDisabled = false,
  chatboxDisabled,
  chatboxButtonRef,
  onPageChange,
  onPageIntent,
  onCompact,
  onChatbox,
  onCapture,
  onNetworkCapture,
  onVision,
  visionDisabled,
  updateAvailable = false,
}: {
  learningAvailable: boolean;
  chatboxAvailable: boolean;
  page: Page;
  running: boolean;
  captureDisabled: boolean;
  captureReason?: string | null;
  networkCaptureVisible?: boolean;
  networkCaptureRunning?: boolean;
  networkCaptureDisabled?: boolean;
  chatboxOpen: boolean;
  chatboxDisabled: boolean;
  chatboxButtonRef: Ref<HTMLButtonElement>;
  onPageChange: (page: Page) => void;
  onPageIntent?: (page: Page) => void;
  onCompact: () => void;
  onChatbox: () => void;
  onCapture: () => void;
  onNetworkCapture?: () => void;
  onVision?: () => void;
  visionDisabled?: boolean;
  updateAvailable?: boolean;
}) {
  const { t } = useTranslation();
  const networkLabel = t(networkCaptureRunning ? "capture.networkStop" : "capture.networkStart");
  return (
    <nav className={`bottom-dock ${chatboxOpen ? "chatbox-open" : ""}`} aria-label={t("navigation.main")}>
      <DockButton label={t("navigation.live")} active={page === "live"} onClick={() => onPageChange("live")}><MessageSquare /></DockButton>
      {learningAvailable && <DockButton label={t("navigation.learning")} active={page === "learning"} onIntent={() => onPageIntent?.("learning")} onClick={() => onPageChange("learning")}><GraduationCap /></DockButton>}
      <DockButton label={t("navigation.settings")} active={page === "settings"} badge={updateAvailable} onIntent={() => onPageIntent?.("settings")} onClick={() => onPageChange("settings")}><SlidersHorizontal /></DockButton>
      <i className="dock-divider" aria-hidden="true" />
      <DockButton label={t("navigation.compact")} tonal onClick={onCompact}><Shrink /></DockButton>
      {chatboxAvailable && <DockButton
        label={t("navigation.chatbox")}
        active={chatboxOpen}
        disabled={chatboxDisabled}
        buttonRef={chatboxButtonRef}
        expanded={chatboxOpen}
        onClick={onChatbox}
      ><MessageSquarePlus /></DockButton>}
      {onVision && <DockButton label={t("visionWindow.title")} disabled={visionDisabled} onClick={onVision}><Eye /></DockButton>}
      {networkCaptureVisible && onNetworkCapture && (
        <DockButton
          label={networkLabel}
          active={networkCaptureRunning}
          disabled={networkCaptureDisabled}
          onClick={onNetworkCapture}
        >
          <Radar />
        </DockButton>
      )}
      <DockButton label={t(running ? "capture.stop" : "capture.start")} tooltip={!running && captureReason ? t(captureReason) : undefined} primary disabled={captureDisabled} onClick={onCapture}>{running ? <Square /> : <Mic />}</DockButton>
    </nav>
  );
}

function DockButton({ label, tooltip, active = false, tonal = false, primary = false, disabled = false, badge = false, expanded, buttonRef, onIntent, onClick, children }: {
  label: string;
  tooltip?: string;
  active?: boolean;
  tonal?: boolean;
  primary?: boolean;
  disabled?: boolean;
  badge?: boolean;
  expanded?: boolean;
  buttonRef?: Ref<HTMLButtonElement>;
  onIntent?: () => void;
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
      data-tooltip={tooltip ?? label}
      disabled={disabled}
      onFocus={onIntent}
      onPointerDown={onIntent}
      onPointerEnter={onIntent}
      onClick={onClick}
    >
      {children}
      {badge && <span className="dock-button-badge" aria-hidden="true" />}
    </button>
  );
}
