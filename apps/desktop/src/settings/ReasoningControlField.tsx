import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ApiModelDetails } from "../providers/types";
import { normalizeReasoningControlForModel, reasoningOptionsForModel } from "../translation/translation-thinking";
import type { ReasoningSetting } from "../translation/translation-thinking";
import { PreferenceToggle, Select } from "./SettingsControls";

export function ReasoningControlField({ provider, model, details, thinkingEnabled, value, disabled = false, onChange }: {
  provider?: string;
  model: string;
  details?: ApiModelDetails;
  thinkingEnabled: boolean;
  value?: ReasoningSetting | null;
  disabled?: boolean;
  onChange: (patch: { thinking_enabled?: boolean; reasoning_control?: ReasoningSetting | null }) => void;
}) {
  const { t } = useTranslation();
  const options = reasoningOptionsForModel(provider, model, details);
  const [budgetText, setBudgetText] = useState("");
  const [invalidBudget, setInvalidBudget] = useState(false);
  useEffect(() => {
    setBudgetText(value?.type === "budget_tokens" && value.value !== -1 ? String(value.value) : "");
    setInvalidBudget(false);
  }, [value?.type, value?.value]);
  const active = thinkingEnabled || options.thinking === "hide_only";
  const valid = !value || normalizeReasoningControlForModel(provider, model, value, details) != null;
  const adjustable = options.efforts.length > 0 || options.budget != null;
  const showStrength = (active || !valid) && Boolean(adjustable || value);
  const selected = value?.type === "effort" ? `effort:${value.value}` : value?.type === "budget_tokens" ? value.value === -1 ? "automatic" : "budget" : "default";
  if (!provider || !model) return null;
  if (options.thinking === "unsupported" && !adjustable && !value) {
    return details?.reasoning?.supported === false ? null : <small>{t("settings.reasoning.unknown")}</small>;
  }
  return <div className={`field reasoning-control-field ${options.thinking === "disable_supported" && showStrength ? "reasoning-control-paired" : ""}`}>
    {options.thinking === "disable_supported" && <PreferenceToggle title={t("settings.translation.thinkingMode")}
      checked={thinkingEnabled} disabled={disabled} onChange={(thinking_enabled) => onChange({ thinking_enabled })} />}
    {options.thinking === "hide_only" && <small>{t("settings.reasoning.mandatory")}</small>}
    {showStrength && <>
      <Select label={t("settings.reasoning.strength")} value={selected} disabled={disabled}
        options={[
          { value: "default", label: t("settings.reasoning.applicationDefault") },
          ...options.efforts.map((effort) => ({ value: `effort:${effort}`, label: t(`settings.reasoning.efforts.${effort}`, { defaultValue: effort }) })),
          ...(options.budget ? [{ value: "budget", label: t("settings.reasoning.budget") }] : []),
          ...(options.budget?.dynamic ? [{ value: "automatic", label: t("settings.reasoning.automatic") }] : []),
          ...(!valid ? [{ value: selected, label: t("settings.reasoning.unavailable") }] : []),
        ]}
        onChange={(next) => onChange({ reasoning_control: next.startsWith("effort:") ? { type: "effort", value: next.slice(7) }
          : next === "budget" ? { type: "budget_tokens", value: options.budget?.min ?? 1 }
          : next === "automatic" ? { type: "budget_tokens", value: -1 } : null })} />
      {selected === "budget" && options.budget && <label className="field reasoning-control-budget">
        <span>{t("settings.reasoning.budgetTokens")}</span>
        <input type="number" aria-label={t("settings.reasoning.budgetTokens")} min={options.budget.min} max={options.budget.max ?? undefined}
          step={1} value={budgetText} disabled={disabled} aria-invalid={invalidBudget}
          onChange={(event) => { setInvalidBudget(false); setBudgetText(event.target.value); }}
          onBlur={() => {
            const next: ReasoningSetting = { type: "budget_tokens", value: Number(budgetText) };
            if (budgetText && normalizeReasoningControlForModel(provider, model, next, details)) onChange({ reasoning_control: next });
            else setInvalidBudget(true);
          }} />
        <small>{t("settings.reasoning.budgetRange", { min: options.budget.min, max: options.budget.max ?? "—" })}</small>
        {invalidBudget && <small role="alert">{t("settings.reasoning.invalidBudget")}</small>}
      </label>}
      {!valid && <small role="alert">{t("settings.reasoning.unavailable")}</small>}
    </>}
  </div>;
}
