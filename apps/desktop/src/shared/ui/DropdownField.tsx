import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown, Search } from "lucide-react";

import { useDismissibleLayer } from "../hooks/use-dismissible-layer";
import { useDropdownMenu } from "../hooks/use-dropdown-menu";

export interface DropdownOption {
  value: string;
  label: string;
  description?: string;
  searchText?: string;
  group?: string;
}

export function DropdownField({ label, value, options, disabled = false, compact = false, floating = false, floatingLayer = "page", floatingWidth, searchable = false, searchPlaceholder, emptyLabel, createOption, icon, placeholder, onChange }: {
  label: string;
  value: string;
  options: DropdownOption[];
  disabled?: boolean;
  compact?: boolean;
  floating?: boolean;
  floatingLayer?: "page" | "dialog";
  floatingWidth?: number;
  searchable?: boolean;
  searchPlaceholder?: string;
  emptyLabel?: string;
  createOption?: (query: string) => DropdownOption | null;
  icon?: ReactNode;
  placeholder?: string;
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const {
    menuRef,
    menuMounted,
    floatingMenuMounted,
    floatingPosition,
    floatingSide,
  } = useDropdownMenu(open, { floating, floatingWidth, anchorRef: triggerRef, onClose: () => setOpen(false) });
  const searchRef = useRef<HTMLInputElement | null>(null);
  const focusMenuOnOpenRef = useRef(false);
  const menuId = useId();
  const matched = options.find((option) => option.value === value);
  // With a placeholder, an unmatched value shows placeholder text instead of the first option.
  const selected = placeholder ? matched : matched ?? options[0];
  const filteredOptions = filterDropdownOptions(options, query);
  const createdOption = createOption?.(query) ?? null;
  const visibleOptions = createdOption
    && !filteredOptions.some((option) => option.value === createdOption.value)
    ? [...filteredOptions, createdOption]
    : filteredOptions;

  useDismissibleLayer(open, rootRef, () => setOpen(false), menuRef);

  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);

  useEffect(() => {
    if (!menuMounted) setQuery("");
  }, [menuMounted]);

  const choose = (next: string) => {
    onChange(next);
    setOpen(false);
    requestAnimationFrame(() => triggerRef.current?.focus({ preventScroll: true }));
  };

  useLayoutEffect(() => {
    if (!open || !focusMenuOnOpenRef.current || !menuRef.current) return;
    focusMenuOnOpenRef.current = false;
    if (searchable && searchRef.current) {
      searchRef.current.focus({ preventScroll: true });
      return;
    }
    const current = menuRef.current.querySelector<HTMLElement>('[aria-selected="true"]');
    (current ?? menuRef.current.querySelector<HTMLElement>("button"))?.focus({ preventScroll: true });
  }, [floatingPosition, open, searchable]);

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
      )).filter((element) => element.getClientRects().length > 0 && !element.closest("[inert]"));
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

  const optionList = (
    <>
      {visibleOptions.map((option, index) => {
        const current = option.value === value;
        const showGroup = Boolean(option.group && option.group !== visibleOptions[index - 1]?.group);
        return (
          <div className="dropdown-option-entry" key={option.value}>
            {showGroup && <div className="dropdown-option-group" role="presentation">{option.group}</div>}
            <button
              className={`dropdown-option ${current ? "selected" : ""}`}
              type="button"
              role="option"
              aria-selected={current}
              onClick={() => choose(option.value)}
            >
              <span className="dropdown-option-copy">
                <span>{option.label}</span>
                {option.description && <small>{option.description}</small>}
              </span>
              {current && <Check size={15} />}
            </button>
          </div>
        );
      })}
      {!visibleOptions.length && <p className="dropdown-empty">{emptyLabel}</p>}
    </>
  );

  const menu = (
    <div
      className={`dropdown-menu ${searchable ? "dropdown-menu-searchable" : ""} ${floating ? "dropdown-menu-floating" : ""} ${floating && floatingLayer === "dialog" ? "dropdown-menu-floating-dialog" : ""} ${compact ? "dropdown-menu-compact" : ""}`}
      id={searchable ? undefined : menuId}
      ref={menuRef}
      role={searchable ? undefined : "listbox"}
      aria-label={searchable ? undefined : label}
      aria-hidden={!open}
      inert={!open}
      data-side={floating ? floatingSide : undefined}
      style={floating ? floatingPosition ?? undefined : undefined}
      onKeyDown={handleMenuKeyDown}
    >
      {searchable && (
        <div className="dropdown-search-wrap">
          <Search className="dropdown-search-icon" size={14} aria-hidden="true" />
          <input
            ref={searchRef}
            className="dropdown-search"
            type="search"
            value={query}
            aria-label={searchPlaceholder ?? label}
            placeholder={searchPlaceholder}
            autoComplete="off"
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && visibleOptions[0]) {
                event.preventDefault();
                choose(visibleOptions[0].value);
              }
            }}
          />
        </div>
      )}
      {searchable
        ? <div className="dropdown-options" id={menuId} role="listbox" aria-label={label}>{optionList}</div>
        : optionList}
    </div>
  );

  return (
    <div
      className={`dropdown-field ${compact ? "dropdown-field-compact" : ""} ${open ? "open" : ""} ${menuMounted ? "menu-visible" : ""}`}
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
        aria-label={label}
        onClick={() => setOpen((current) => {
          const next = !current;
          if (next && searchable) focusMenuOnOpenRef.current = true;
          return next;
        })}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp" || event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            focusMenuOnOpenRef.current = true;
            setOpen(true);
          }
        }}
      >
        {icon && <span className="dropdown-icon">{icon}</span>}
        <span className="dropdown-value">{selected?.label ?? placeholder ?? value}</span>
        <ChevronDown className="dropdown-chevron" size={16} />
      </button>
      {menuMounted && (!floating || floatingPosition) && (
        floating ? createPortal(menu, document.body) : menu
      )}
    </div>
  );
}

function normalizeSearch(value: string): string {
  return value
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLocaleLowerCase()
    .trim();
}

function filterDropdownOptions(options: DropdownOption[], query: string): DropdownOption[] {
  const normalizedQuery = normalizeSearch(query);
  return normalizedQuery
    ? options.filter((option) => normalizeSearch(
        `${option.label} ${option.description ?? ""} ${option.searchText ?? ""} ${option.value}`,
      ).includes(normalizedQuery))
    : options;
}

export function EditableDropdownField({
  label,
  value,
  options,
  disabled = false,
  optionsDisabled = false,
  placeholder,
  commitOnBlur = false,
  floating = true,
  onChange,
}: {
  label: string;
  value: string;
  options: DropdownOption[];
  disabled?: boolean;
  optionsDisabled?: boolean;
  placeholder?: string;
  commitOnBlur?: boolean;
  floating?: boolean;
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [activeValue, setActiveValue] = useState<string | null>(null);
  const [draftValue, setDraftValue] = useState(value);
  const submittedValueRef = useRef(value);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const controlRef = useRef<HTMLDivElement | null>(null);
  const { menuRef, menuMounted, floatingPosition, floatingSide } = useDropdownMenu(open, {
    floating,
    anchorRef: controlRef,
    onClose: () => setOpen(false),
  });
  const menuId = useId();
  const listDisabled = disabled || optionsDisabled || options.length === 0;
  const visibleOptions = filterDropdownOptions(options, query);
  const activeIndex = visibleOptions.findIndex((option) => option.value === activeValue);
  const activeId = open && activeIndex >= 0 ? `${menuId}-option-${activeIndex}` : undefined;

  const closeMenu = () => {
    setOpen(false);
    setActiveValue(null);
  };

  useEffect(() => {
    setDraftValue(value);
    submittedValueRef.current = value;
  }, [value]);

  useDismissibleLayer(open, rootRef, closeMenu, menuRef);

  useEffect(() => {
    if (listDisabled || !visibleOptions.length) closeMenu();
  }, [listDisabled, visibleOptions.length]);

  useLayoutEffect(() => {
    if (activeId) document.getElementById(activeId)?.scrollIntoView({ block: "nearest" });
  }, [activeId, floatingPosition]);

  const choose = (next: string) => {
    setDraftValue(next);
    submittedValueRef.current = next;
    onChange(next);
    closeMenu();
    inputRef.current?.focus({ preventScroll: true });
  };

  const menu = (
    <div
      className={`dropdown-menu editable-dropdown-menu ${floating ? "dropdown-menu-floating" : ""}`}
      id={menuId}
      ref={menuRef}
      role="listbox"
      aria-label={label}
      aria-hidden={!open}
      inert={!open}
      data-side={floating ? floatingSide : undefined}
      style={floating ? floatingPosition ?? undefined : undefined}
    >
      {visibleOptions.map((option, index) => {
        const current = option.value === value;
        return (
          <button
            className={`dropdown-option ${current ? "selected" : ""}`}
            id={`${menuId}-option-${index}`}
            key={option.value}
            type="button"
            role="option"
            tabIndex={-1}
            aria-selected={current}
            data-active={index === activeIndex ? "true" : undefined}
            onMouseDown={(event) => {
              if (event.button === 0) event.preventDefault();
            }}
            onClick={() => choose(option.value)}
          >
            <span className="dropdown-option-copy">
              <span>{option.label}</span>
              {option.description && <small>{option.description}</small>}
            </span>
            {current && <Check size={15} />}
          </button>
        );
      })}
    </div>
  );

  return (
    <div
      className={`dropdown-field editable-dropdown-field ${open ? "open" : ""} ${menuMounted ? "menu-visible" : ""}`}
      ref={rootRef}
      onBlur={(event) => {
        if (event.currentTarget.contains(event.relatedTarget)) return;
        if (menuRef.current?.contains(event.relatedTarget)) return;
        closeMenu();
        if (!commitOnBlur) return;
        if (draftValue === submittedValueRef.current) return;
        submittedValueRef.current = draftValue;
        onChange(draftValue);
      }}
      onKeyDownCapture={(event) => {
        if (event.key !== "Escape" || !open) return;
        event.stopPropagation();
        if (event.nativeEvent.isComposing) return;
        event.preventDefault();
        closeMenu();
      }}
    >
      <div className="editable-dropdown-control" ref={controlRef}>
        <input
          ref={inputRef}
          role="combobox"
          aria-autocomplete="list"
          aria-expanded={open}
          aria-controls={menuId}
          aria-activedescendant={activeId}
          aria-label={label}
          value={commitOnBlur ? draftValue : value}
          placeholder={placeholder}
          disabled={disabled}
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => {
            const next = event.target.value;
            setDraftValue(next);
            setQuery(next);
            setActiveValue(null);
            setOpen(!listDisabled && filterDropdownOptions(options, next).length > 0);
            if (!commitOnBlur) onChange(next);
          }}
          onKeyDown={(event) => {
            if (event.nativeEvent.isComposing) return;
            if ((event.key === "ArrowDown" || event.key === "ArrowUp") && !listDisabled && visibleOptions.length) {
              event.preventDefault();
              const current = open ? activeIndex : -1;
              const next = event.key === "ArrowDown"
                ? (current + 1) % visibleOptions.length
                : (current < 0 ? visibleOptions.length - 1 : (current - 1 + visibleOptions.length) % visibleOptions.length);
              setActiveValue(visibleOptions[next].value);
              setOpen(true);
            } else if (event.key === "Enter" && open && activeIndex >= 0) {
              event.preventDefault();
              choose(visibleOptions[activeIndex].value);
            } else if (commitOnBlur && event.key === "Enter") {
              event.preventDefault();
              event.currentTarget.blur();
            }
          }}
        />
        <button
          className="editable-dropdown-toggle"
          type="button"
          disabled={listDisabled}
          aria-label={label}
          aria-haspopup="listbox"
          aria-expanded={open}
          aria-controls={menuId}
          onClick={() => {
            setQuery("");
            setActiveValue(null);
            setOpen((current) => !current);
            inputRef.current?.focus({ preventScroll: true });
          }}
        >
          <ChevronDown className="dropdown-chevron" size={16} />
        </button>
      </div>
      {menuMounted && (!floating || floatingPosition) && (
        floating ? createPortal(menu, document.body) : menu
      )}
    </div>
  );
}
