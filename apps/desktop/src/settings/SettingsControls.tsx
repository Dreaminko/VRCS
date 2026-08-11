import { useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Check, ChevronDown, ChevronRight } from "lucide-react";

import {
  ankiDeckAncestors,
  ankiDeckDisplayName,
  ankiDeckParent,
  buildAnkiDeckTree,
  visibleAnkiDeckNodes,
} from "../anki-decks";
import type { AudioDevice } from "../types";
import { useDismissibleLayer } from "../use-dismissible-layer";
import { DropdownField } from "../components/DropdownField";

export function PreferenceToggle({ title, description, checked, disabled, onChange }: {
  title: string;
  description: string;
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <div className={`settings-toggle-row ${disabled ? "disabled" : ""}`}>
      <span className="settings-toggle-copy">
        <strong>{title}</strong>
        <small>{description}</small>
      </span>
      <button
        className="settings-switch-button"
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={title}
        disabled={disabled}
        onClick={() => onChange(!checked)}
      >
        <span className="switch-track" aria-hidden="true"><span /></span>
      </button>
    </div>
  );
}

export function Select({ label, helper, value, values = [], options, disabled, onChange }: {
  label: string;
  helper?: string;
  value: string;
  values?: readonly string[];
  options?: Array<{ value: string; label: string }>;
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <div className="field">
      <span>{label}</span>
      <DropdownField
        label={label}
        value={value}
        options={options ?? values.map((item) => ({ value: item, label: item }))}
        disabled={disabled}
        onChange={onChange}
      />
      {helper && <small>{helper}</small>}
    </div>
  );
}

export function RangeField({ label, helper, value, min, max, step, disabled, formatValue, onCommit, hideValue, hideBounds, trackSlot }: {
  label: string;
  helper: string;
  value: number;
  min: number;
  max: number;
  step: number;
  disabled: boolean;
  formatValue: (value: number) => string;
  onCommit: (value: number) => void;
  hideValue?: boolean;
  hideBounds?: boolean;
  trackSlot?: ReactNode;
}) {
  const { t } = useTranslation();
  const [draftValue, setDraftValue] = useState(value);
  const draftValueRef = useRef(value);
  const committedValueRef = useRef(value);
  const progress = ((draftValue - min) / (max - min)) * 100;

  useEffect(() => {
    draftValueRef.current = value;
    committedValueRef.current = value;
    setDraftValue(value);
  }, [value]);

  const commit = () => {
    const next = draftValueRef.current;
    if (next === committedValueRef.current) return;
    committedValueRef.current = next;
    onCommit(next);
  };

  return (
    <label className={`range-field ${disabled ? "disabled" : ""}`}>
      <span className="range-field-header">
        <span>{label}</span>
        {!hideValue && <output aria-label={t("common.currentValue", { label })}>{formatValue(draftValue)}</output>}
      </span>
      <span className="range-input-wrap">
        {trackSlot}
        <input
          className="range-input"
          type="range"
          min={min}
          max={max}
          step={step}
          value={draftValue}
          disabled={disabled}
          aria-label={label}
          aria-valuetext={formatValue(draftValue)}
          style={{ "--range-progress": `${progress}%` } as CSSProperties}
          onChange={(event) => {
            const next = Number(event.target.value);
            draftValueRef.current = next;
            setDraftValue(next);
          }}
          onPointerUp={commit}
          onPointerCancel={commit}
          onKeyUp={commit}
          onBlur={commit}
        />
      </span>
      {!hideBounds && (
        <span className="range-bounds" aria-hidden="true">
          <span>{formatValue(min)}</span>
          <span>{formatValue(max)}</span>
        </span>
      )}
      <small>{helper}</small>
    </label>
  );
}

export function DeckTreeSelect({ label, helper, value, decks, disabled, onChange }: {
  label: string;
  helper?: string;
  value: string;
  decks: string[];
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const [open, setOpen] = useState(false);
  const [expandedNames, setExpandedNames] = useState<Set<string>>(
    () => new Set(ankiDeckAncestors(value)),
  );
  const [activeName, setActiveName] = useState(value);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const itemRefs = useRef(new Map<string, HTMLButtonElement>());
  const tree = useMemo(() => buildAnkiDeckTree(decks, locale), [decks, locale]);
  const visibleNodes = useMemo(
    () => visibleAnkiDeckNodes(tree, expandedNames),
    [tree, expandedNames],
  );

  const closeAndFocusTrigger = () => {
    setOpen(false);
    requestAnimationFrame(() => triggerRef.current?.focus());
  };

  const openTree = () => {
    setExpandedNames((current) => new Set([
      ...current,
      ...ankiDeckAncestors(value),
    ]));
    setActiveName(value || tree[0]?.name || "");
    setOpen(true);
  };

  const toggleExpanded = (name: string) => {
    setExpandedNames((current) => {
      const next = new Set(current);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  };

  const choose = (name: string) => {
    onChange(name);
    setActiveName(name);
    closeAndFocusTrigger();
  };

  useDismissibleLayer(open, rootRef, closeAndFocusTrigger);

  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);

  useEffect(() => {
    if (!open) return;
    const frame = requestAnimationFrame(() => itemRefs.current.get(activeName)?.focus());
    return () => cancelAnimationFrame(frame);
  }, [activeName, open]);

  return (
    <div className="field">
      <span>{label}</span>
      <div className={`deck-tree-field ${open ? "open" : ""}`} ref={rootRef}>
        <button
          className="dropdown-trigger deck-tree-trigger"
          type="button"
          ref={triggerRef}
          disabled={disabled}
          aria-haspopup="tree"
          aria-expanded={open}
          aria-controls="anki-deck-tree"
          onClick={() => {
            if (open) setOpen(false);
            else openTree();
          }}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown" || event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              openTree();
            }
          }}
        >
          <span className="dropdown-value">{ankiDeckDisplayName(value)}</span>
          <ChevronDown className="dropdown-chevron" size={16} />
        </button>
        {open && (
          <div className="deck-tree-menu">
            <div
              className="deck-tree-list"
              id="anki-deck-tree"
              role="tree"
              aria-label={label}
            >
              {visibleNodes.map((node, index) => {
                const current = node.name === value;
                return (
                  <div
                    className={`deck-tree-row ${current ? "selected" : ""}`}
                    key={node.name}
                    role="none"
                    style={{ "--deck-indent": `${(node.depth - 1) * 16}px` } as CSSProperties}
                  >
                    {node.hasChildren ? (
                      <button
                        className="deck-tree-toggle"
                        type="button"
                        tabIndex={-1}
                        aria-label={t(
                          node.expanded ? "common.collapseNamed" : "common.expandNamed",
                          { name: node.label },
                        )}
                        onClick={() => toggleExpanded(node.name)}
                      >
                        <ChevronRight className={node.expanded ? "expanded" : ""} size={15} />
                      </button>
                    ) : (
                      <span className="deck-tree-leaf-space" aria-hidden="true" />
                    )}
                    <button
                      className={`deck-tree-item ${node.selectable ? "" : "group-only"}`}
                      type="button"
                      role="treeitem"
                      aria-level={node.depth}
                      aria-expanded={node.hasChildren ? node.expanded : undefined}
                      aria-selected={current}
                      tabIndex={node.name === activeName ? 0 : -1}
                      ref={(element) => {
                        if (element) itemRefs.current.set(node.name, element);
                        else itemRefs.current.delete(node.name);
                      }}
                      onClick={() => {
                        if (node.selectable) choose(node.name);
                        else toggleExpanded(node.name);
                      }}
                      onKeyDown={(event) => {
                        if (event.key === "ArrowDown") {
                          event.preventDefault();
                          setActiveName(visibleNodes[Math.min(index + 1, visibleNodes.length - 1)].name);
                        } else if (event.key === "ArrowUp") {
                          event.preventDefault();
                          setActiveName(visibleNodes[Math.max(index - 1, 0)].name);
                        } else if (event.key === "Home") {
                          event.preventDefault();
                          setActiveName(visibleNodes[0].name);
                        } else if (event.key === "End") {
                          event.preventDefault();
                          setActiveName(visibleNodes[visibleNodes.length - 1].name);
                        } else if (event.key === "ArrowRight" && node.hasChildren) {
                          event.preventDefault();
                          if (!node.expanded) toggleExpanded(node.name);
                          else if (visibleNodes[index + 1]) setActiveName(visibleNodes[index + 1].name);
                        } else if (event.key === "ArrowLeft") {
                          event.preventDefault();
                          if (node.expanded) toggleExpanded(node.name);
                          else {
                            const parent = ankiDeckParent(node.name);
                            if (parent) setActiveName(parent);
                          }
                        } else if (event.key === "Enter" || event.key === " ") {
                          event.preventDefault();
                          if (node.selectable) choose(node.name);
                          else toggleExpanded(node.name);
                        }
                      }}
                    >
                      <span>{node.label}</span>
                      {current && <Check size={15} />}
                    </button>
                  </div>
                );
              })}
            </div>
          </div>
        )}
      </div>
      {helper && <small>{helper}</small>}
    </div>
  );
}

export function DeviceGroup({ icon, title, note, beforeList, devices, devicesReady, selectedDeviceId, specialRows, disabled, onSelectDevice }: {
  icon: ReactNode;
  title: string;
  note?: string;
  beforeList?: ReactNode;
  devices: AudioDevice[];
  devicesReady: boolean;
  selectedDeviceId: number | null;
  specialRows: Array<{
    key: string;
    name: string;
    description: string;
    chosen: boolean;
    onSelect: () => void;
  }>;
  disabled: boolean;
  onSelectDevice: (id: number) => void;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  return (
    <div className="device-group">
      <div className="device-group-title">
        <span className="device-group-icon">{icon}</span>
        <div><h3>{title}</h3>{note && <span>{note}</span>}</div>
      </div>
      {beforeList}
      <div className="device-list">
        {specialRows.map((row) => (
          <DeviceRow
            key={row.key}
            name={row.name}
            description={row.description}
            chosen={row.chosen}
            disabled={disabled}
            onSelect={row.onSelect}
          />
        ))}
        {devices.map((device) => (
          <DeviceRow
            key={device.id}
            name={device.name}
            description={t("settings.audio.deviceDescription", {
              sampleRate: new Intl.NumberFormat(locale).format(device.sample_rate),
              channels: device.channels,
              defaultSuffix: device.is_default ? t("settings.audio.defaultSuffix") : "",
            })}
            chosen={selectedDeviceId === device.id}
            disabled={disabled}
            onSelect={() => onSelectDevice(device.id)}
          />
        ))}
        {!devicesReady
          ? <p className="device-empty">{t("settings.audio.scanning")}</p>
          : !devices.length && <p className="device-empty">{t("settings.audio.noDevices")}</p>}
      </div>
    </div>
  );
}

function DeviceRow({ name, description, chosen, disabled, onSelect }: {
  name: string;
  description: string;
  chosen: boolean;
  disabled: boolean;
  onSelect: () => void;
}) {
  return (
    <label className={`device-row ${chosen ? "chosen" : ""} ${disabled ? "disabled" : ""}`}>
      <input type="radio" aria-label={name} checked={chosen} disabled={disabled} onChange={onSelect} />
      <span><strong>{name}</strong><small>{description}</small></span>
    </label>
  );
}
