import { X } from "lucide-react";
import { useEffect, useRef, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

const FOCUSABLE_SELECTOR = [
  "button:not(:disabled)",
  "input:not(:disabled):not([type=\"hidden\"])",
  "textarea:not(:disabled)",
  "select:not(:disabled)",
  "[tabindex]:not([tabindex=\"-1\"])",
].join(", ");

// Nested dialogs (for example a confirmation above an editor) only react to keys when they are on top.
const dialogStack: HTMLElement[] = [];
let openDialogCount = 0;
let previousBodyOverflow = "";

export function SettingsDialog({
  label,
  saving,
  returnFocusRef,
  className,
  autoFocus = false,
  onClose,
  children,
}: {
  label: string;
  saving: boolean;
  returnFocusRef: RefObject<HTMLButtonElement | null>;
  className?: string;
  autoFocus?: boolean;
  onClose: () => void;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  const dialogRef = useRef<HTMLElement>(null);
  const closeRef = useRef(onClose);
  const savingRef = useRef(saving);

  useEffect(() => {
    closeRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    savingRef.current = saving;
  }, [saving]);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (dialog) dialogStack.push(dialog);
    if (openDialogCount === 0) previousBodyOverflow = document.body.style.overflow;
    openDialogCount += 1;
    document.body.style.overflow = "hidden";
    const frame = autoFocus
      ? window.requestAnimationFrame(() => {
          dialogRef.current?.querySelector<HTMLElement>(FOCUSABLE_SELECTOR)?.focus();
        })
      : null;

    const handleKeyDown = (event: KeyboardEvent) => {
      if (dialogStack[dialogStack.length - 1] !== dialogRef.current) return;
      if (event.key === "Escape") {
        event.preventDefault();
        if (!savingRef.current) closeRef.current();
        return;
      }
      if (event.key !== "Tab" || !dialogRef.current) return;

      const focusable = Array.from(
        dialogRef.current.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR),
      ).filter((element) => element.getClientRects().length > 0 && !element.closest("[inert]"));
      if (!focusable.length) return;

      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && (
        document.activeElement === first
        || !dialogRef.current.contains(document.activeElement)
      )) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && (
        document.activeElement === last
        || !dialogRef.current.contains(document.activeElement)
      )) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => {
      if (frame !== null) window.cancelAnimationFrame(frame);
      document.removeEventListener("keydown", handleKeyDown);
      const index = dialog ? dialogStack.lastIndexOf(dialog) : -1;
      if (index !== -1) dialogStack.splice(index, 1);
      openDialogCount -= 1;
      if (openDialogCount === 0) document.body.style.overflow = previousBodyOverflow;
      returnFocusRef.current?.focus();
    };
  }, [autoFocus, returnFocusRef]);

  return createPortal(
    <div
      className="api-profile-dialog-backdrop"
      onMouseDown={(event) => {
        if (!saving && event.target === event.currentTarget) onClose();
      }}
    >
      <section
        className={["api-profile-dialog", className].filter(Boolean).join(" ")}
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label={label}
      >
        <button
          className="api-profile-dialog-close"
          type="button"
          aria-label={t("common.close")}
          disabled={saving}
          onClick={onClose}
        >
          <X size={18} aria-hidden="true" />
        </button>
        {children}
      </section>
    </div>,
    document.body,
  );
}
