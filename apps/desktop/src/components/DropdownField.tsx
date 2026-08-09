import { useEffect, useId, useRef, useState } from "react";
import type { ReactNode } from "react";
import { Check, ChevronDown } from "lucide-react";

import { useDismissibleLayer } from "../use-dismissible-layer";

export function DropdownField({ label, value, options, disabled = false, compact = false, icon, onChange }: {
  label: string;
  value: string;
  options: Array<{ value: string; label: string }>;
  disabled?: boolean;
  compact?: boolean;
  icon?: ReactNode;
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const selected = options.find((option) => option.value === value) ?? options[0];

  useDismissibleLayer(open, rootRef, () => setOpen(false));

  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);

  const choose = (next: string) => {
    onChange(next);
    setOpen(false);
  };

  return (
    <div className={`dropdown-field ${compact ? "dropdown-field-compact" : ""} ${open ? "open" : ""}`} ref={rootRef}>
      <button
        className="dropdown-trigger"
        type="button"
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={compact ? label : undefined}
        onClick={() => setOpen((current) => !current)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            setOpen(true);
          }
        }}
      >
        {icon && <span className="dropdown-icon">{icon}</span>}
        <span className="dropdown-value">{selected?.label ?? value}</span>
        <ChevronDown className="dropdown-chevron" size={16} />
      </button>
      {open && (
        <div className="dropdown-menu" role="listbox" aria-label={label}>
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
