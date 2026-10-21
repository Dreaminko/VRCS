import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { Blend, SquareDashed, type LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";

export function CompactBackgroundControls({ transparentBackground, onToggleBackground, opacity, onOpacityChange }: {
  transparentBackground: boolean;
  onToggleBackground: () => void;
  opacity: number;
  onOpacityChange: (value: number) => void;
}) {
  const { t } = useTranslation();
  const modeLabel = t(transparentBackground ? "window.backgroundMode" : "window.subtitleOnlyMode");

  return (
    <>
      {!transparentBackground && <CompactOpacityControl value={opacity} onChange={onOpacityChange} />}
      <button type="button" aria-label={modeLabel} title={modeLabel} aria-pressed={transparentBackground} onClick={onToggleBackground}>
        <CompactWindowIcon icon={SquareDashed} />
      </button>
    </>
  );
}

function CompactOpacityControl({ value, onChange }: { value: number; onChange: (value: number) => void }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const controlRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const sliderRef = useRef<HTMLInputElement>(null);
  const id = useId();
  const label = t("window.backgroundOpacity");
  const currentLabel = t("window.backgroundOpacityCurrent", { percent: value });
  const close = useCallback(() => {
    setOpen(false);
    buttonRef.current?.focus({ preventScroll: true });
  }, []);

  useLayoutEffect(() => {
    if (open) sliderRef.current?.focus({ preventScroll: true });
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      if (!controlRef.current?.contains(event.target as Node)) close();
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      close();
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", escape, true);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("keydown", escape, true);
    };
  }, [open, close]);

  return (
    <div ref={controlRef} className="compact-opacity-control" onBlur={(event) => {
      if (!event.currentTarget.contains(event.relatedTarget)) setOpen(false);
    }}>
      <button ref={buttonRef} type="button" aria-label={currentLabel} title={currentLabel}
        aria-haspopup="dialog" aria-expanded={open} aria-controls={open ? id : undefined}
        onClick={() => setOpen((previous) => !previous)} onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp") { event.preventDefault(); setOpen(true); }
        }}>
        <CompactWindowIcon icon={Blend} />
      </button>
      {open && <div id={id} className="compact-opacity-popover" role="dialog" aria-labelledby={`${id}-label`}
        onKeyDown={(event) => {
          if (event.key === "Tab") { event.preventDefault(); close(); }
        }}>
        <div className="compact-opacity-heading">
          <label id={`${id}-label`} htmlFor={`${id}-slider`}>{label}</label>
          <output htmlFor={`${id}-slider`}>{value}%</output>
        </div>
        <input ref={sliderRef} id={`${id}-slider`} className="range-input compact-opacity-slider" type="range"
          min={30} max={100} step={1} value={value} aria-valuetext={currentLabel}
          style={{ "--range-progress": `${(value - 30) / 70 * 100}%` } as CSSProperties}
          onChange={(event) => onChange(event.currentTarget.valueAsNumber)} />
        <div className="compact-opacity-bounds" aria-hidden="true"><span>30%</span><span>100%</span></div>
      </div>}
    </div>
  );
}

// Paint a wider white copy behind the original strokes only in transparent mode.
export function CompactWindowIcon({ icon: Icon }: { icon: LucideIcon }) {
  return (
    <span className="compact-window-icon" aria-hidden="true">
      <Icon className="compact-icon-outline" size={15} strokeWidth={5.2} />
      <Icon className="compact-icon-foreground" size={15} />
    </span>
  );
}
