import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import type { CSSProperties, KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown } from "lucide-react";

import {
  INTERFACE_LAYOUT_CHANGE_EVENT,
  interfaceLayoutPixels,
  readAppliedInterfaceScaleFactor,
} from "../interface-scale";
import { placeLookupPopover } from "../popover-placement";
import { useDismissibleLayer } from "../use-dismissible-layer";

type FloatingMenuPosition = Pick<CSSProperties, "bottom" | "left" | "maxHeight" | "right" | "top" | "width">;

const FLOATING_MENU_GAP = 6;
const FLOATING_MENU_MARGIN = 12;
const FLOATING_MENU_MAX_HEIGHT = 216;

export function DropdownField({ label, value, options, disabled = false, compact = false, floating = false, floatingLayer = "page", icon, onChange }: {
  label: string;
  value: string;
  options: Array<{ value: string; label: string }>;
  disabled?: boolean;
  compact?: boolean;
  floating?: boolean;
  floatingLayer?: "page" | "dialog";
  icon?: ReactNode;
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [floatingPosition, setFloatingPosition] = useState<FloatingMenuPosition | null>(null);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const focusMenuOnOpenRef = useRef(false);
  const menuId = useId();
  const selected = options.find((option) => option.value === value) ?? options[0];

  useDismissibleLayer(open, rootRef, () => setOpen(false), menuRef);

  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);

  const choose = (next: string) => {
    onChange(next);
    setOpen(false);
    requestAnimationFrame(() => triggerRef.current?.focus({ preventScroll: true }));
  };

  const updateFloatingPosition = useCallback(() => {
    if (!floating || !open || !triggerRef.current) return;

    const scale = readAppliedInterfaceScaleFactor();
    const rect = triggerRef.current.getBoundingClientRect();
    const boundary = triggerRef.current.closest<HTMLElement>("[data-floating-boundary]");
    if (boundary) {
      const boundaryRect = boundary.getBoundingClientRect();
      if (rect.bottom <= boundaryRect.top || rect.top >= boundaryRect.bottom) {
        setOpen(false);
        return;
      }
    }
    const viewportWidth = interfaceLayoutPixels(window.innerWidth, scale);
    const viewportHeight = interfaceLayoutPixels(window.innerHeight, scale);
    const width = Math.min(
      interfaceLayoutPixels(rect.width, scale),
      viewportWidth - FLOATING_MENU_MARGIN * 2,
    );
    const triggerLeft = interfaceLayoutPixels(rect.left, scale);
    const left = Math.min(
      Math.max(FLOATING_MENU_MARGIN, triggerLeft),
      viewportWidth - FLOATING_MENU_MARGIN - width,
    );
    const placement = placeLookupPopover({
      anchor: {
        top: interfaceLayoutPixels(rect.top, scale),
        bottom: interfaceLayoutPixels(rect.bottom, scale),
        centerX: triggerLeft + width / 2,
      },
      popoverHeight: FLOATING_MENU_MAX_HEIGHT,
      viewportHeight,
      gap: FLOATING_MENU_GAP,
      margin: FLOATING_MENU_MARGIN,
    });

    setFloatingPosition({
      bottom: "auto",
      left,
      maxHeight: placement.maxHeight,
      right: "auto",
      top: placement.top,
      width,
    });
  }, [floating, open]);

  useLayoutEffect(() => {
    if (!floating || !open) {
      setFloatingPosition(null);
      return;
    }

    updateFloatingPosition();
    window.addEventListener(INTERFACE_LAYOUT_CHANGE_EVENT, updateFloatingPosition);
    document.addEventListener("scroll", updateFloatingPosition, true);
    return () => {
      window.removeEventListener(INTERFACE_LAYOUT_CHANGE_EVENT, updateFloatingPosition);
      document.removeEventListener("scroll", updateFloatingPosition, true);
    };
  }, [floating, open, updateFloatingPosition]);

  useLayoutEffect(() => {
    if (!open || !focusMenuOnOpenRef.current || !menuRef.current) return;
    focusMenuOnOpenRef.current = false;
    const current = menuRef.current.querySelector<HTMLElement>('[aria-selected="true"]');
    (current ?? menuRef.current.querySelector<HTMLElement>("button"))?.focus({ preventScroll: true });
  }, [floatingPosition, open]);

  const closeMenu = () => {
    setOpen(false);
    requestAnimationFrame(() => triggerRef.current?.focus({ preventScroll: true }));
  };

  const handleMenuKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      closeMenu();
      return;
    }

    if (event.key === "Tab" && floatingLayer === "dialog") {
      const dialog = triggerRef.current?.closest<HTMLElement>('[role="dialog"]');
      if (!dialog || !triggerRef.current) return;
      const focusable = Array.from(dialog.querySelectorAll<HTMLElement>(
        'button:not(:disabled), input:not(:disabled), [tabindex]:not([tabindex="-1"])',
      )).filter((element) => element.getClientRects().length > 0);
      const triggerIndex = focusable.indexOf(triggerRef.current);
      if (triggerIndex < 0 || focusable.length < 2) return;
      event.preventDefault();
      event.stopPropagation();
      const offset = event.shiftKey ? -1 : 1;
      const nextIndex = (triggerIndex + offset + focusable.length) % focusable.length;
      setOpen(false);
      focusable[nextIndex]?.focus({ preventScroll: true });
      return;
    }

    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    const items = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>(".dropdown-option"));
    if (items.length === 0) return;
    event.preventDefault();
    const currentIndex = items.indexOf(document.activeElement as HTMLButtonElement);
    const nextIndex = event.key === "Home"
      ? 0
      : event.key === "End"
        ? items.length - 1
        : event.key === "ArrowUp"
          ? (currentIndex <= 0 ? items.length - 1 : currentIndex - 1)
          : (currentIndex + 1) % items.length;
    items[nextIndex]?.focus({ preventScroll: true });
  };

  const menu = (
    <div
      className={`dropdown-menu ${floating ? "dropdown-menu-floating" : ""} ${floating && floatingLayer === "dialog" ? "dropdown-menu-floating-dialog" : ""} ${compact ? "dropdown-menu-compact" : ""}`}
      id={menuId}
      ref={menuRef}
      role="listbox"
      aria-label={label}
      style={floating ? floatingPosition ?? undefined : undefined}
      onKeyDown={handleMenuKeyDown}
    >
      {options.map((option) => {
        const current = option.value === value;
        return (
          <button
            className={`dropdown-option ${current ? "selected" : ""}`}
            key={option.value}
            type="button"
            role="option"
            aria-selected={current}
            onClick={() => choose(option.value)}
          >
            <span>{option.label}</span>
            {current && <Check size={15} />}
          </button>
        );
      })}
    </div>
  );

  return (
    <div
      className={`dropdown-field ${compact ? "dropdown-field-compact" : ""} ${open ? "open" : ""}`}
      ref={rootRef}
      onBlurCapture={(event) => {
        const next = event.relatedTarget as Node | null;
        if (next && !event.currentTarget.contains(next) && !menuRef.current?.contains(next)) {
          setOpen(false);
        }
      }}
      onKeyDownCapture={(event) => {
        if (event.key !== "Escape" || !open) return;
        event.preventDefault();
        event.stopPropagation();
        closeMenu();
      }}
    >
      <button
        className="dropdown-trigger"
        ref={triggerRef}
        type="button"
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        aria-label={compact ? label : undefined}
        onClick={() => setOpen((current) => !current)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp" || event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            focusMenuOnOpenRef.current = true;
            setOpen(true);
          }
        }}
      >
        {icon && <span className="dropdown-icon">{icon}</span>}
        <span className="dropdown-value">{selected?.label ?? value}</span>
        <ChevronDown className="dropdown-chevron" size={16} />
      </button>
      {open && (!floating || floatingPosition) && (
        floating ? createPortal(menu, document.body) : menu
      )}
    </div>
  );
}

export function EditableDropdownField({
  label,
  value,
  options,
  disabled = false,
  optionsDisabled = false,
  placeholder,
  onChange,
}: {
  label: string;
  value: string;
  options: Array<{ value: string; label: string }>;
  disabled?: boolean;
  optionsDisabled?: boolean;
  placeholder?: string;
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const menuId = useId();
  const listDisabled = disabled || optionsDisabled || options.length === 0;

  useDismissibleLayer(open, rootRef, () => setOpen(false));

  useEffect(() => {
    if (listDisabled) setOpen(false);
  }, [listDisabled]);

  const choose = (next: string) => {
    onChange(next);
    setOpen(false);
    inputRef.current?.focus();
  };

  return (
    <div
      className={`dropdown-field editable-dropdown-field ${open ? "open" : ""}`}
      ref={rootRef}
    >
      <div className="editable-dropdown-control">
        <input
          ref={inputRef}
          role="combobox"
          aria-autocomplete="list"
          aria-expanded={open}
          aria-controls={menuId}
          aria-label={label}
          value={value}
          placeholder={placeholder}
          disabled={disabled}
          onChange={(event) => onChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown" && !listDisabled) {
              event.preventDefault();
              setOpen(true);
            }
            if (event.key === "Escape") setOpen(false);
          }}
        />
        <button
          className="editable-dropdown-toggle"
          type="button"
          disabled={listDisabled}
          aria-label={label}
          aria-haspopup="listbox"
          aria-expanded={open}
          onClick={() => setOpen((current) => !current)}
        >
          <ChevronDown className="dropdown-chevron" size={16} />
        </button>
      </div>
      {open && (
        <div className="dropdown-menu" id={menuId} role="listbox" aria-label={label}>
          {options.map((option) => {
            const current = option.value === value;
            return (
              <button
                className={`dropdown-option ${current ? "selected" : ""}`}
                key={option.value}
                type="button"
                role="option"
                aria-selected={current}
                onClick={() => choose(option.value)}
              >
                <span>{option.label}</span>
                {current && <Check size={15} />}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
