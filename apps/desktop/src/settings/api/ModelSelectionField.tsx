import { useEffect, useRef, useState, type ReactNode } from "react";
import { Pencil, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";

import { profileSupportsCapability } from "../../providers/provider-catalog";
import type { ApiCapability, ApiProfileView } from "../../providers/types";
import { DropdownField, EditableDropdownField } from "../../shared/ui/DropdownField";
import { switchTranslationProfile, translationModels, updateTranslationModel } from "../../translation/translation-model-selection";
import { normalizeReasoningControlForModel } from "../../translation/translation-thinking";
import type { ReasoningSetting } from "../../translation/translation-thinking";
import { SettingsDialog } from "../components/SettingsDialog";
import { useProfileModels } from "../hooks/useProfileModels";
import { resolveModelSelection } from "../model-selection";
import { ReasoningControlField } from "../ReasoningControlField";
import type { ModelBinding, ModelSelection, TranslationTargetSettings } from "../types";
import { visionModels } from "../vision-settings";

export interface ModelChoice extends ModelSelection {
  thinking_enabled?: boolean;
  reasoning_control?: ReasoningSetting | null;
}

export function modelBindingLabel(value: ModelBinding, profiles: ApiProfileView[], empty: string): string {
  if (!value.profile_id) return empty;
  const profile = profiles.find((item) => item.id === value.profile_id);
  return [profile?.name ?? value.profile_id, value.model].filter(Boolean).join(" · ");
}

export function DefaultModelEditor({ value, profiles, capability, onChange }: {
  value: ModelBinding;
  profiles: ApiProfileView[];
  capability: ApiCapability;
  onChange: (value: ModelBinding) => void;
}) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<ModelChoice>(value);
  useEffect(() => setDraft(value), [value.profile_id, value.model]);
  const changed = draft.profile_id !== value.profile_id || draft.model !== value.model;
  return <>
    <ModelBindingEditor value={draft} profiles={profiles} capability={capability} onChange={setDraft} />
    <div className="model-default-actions">
      <button className="primary-button" type="button" disabled={!changed || !draft.profile_id || !draft.model.trim()}
        onClick={() => onChange({ profile_id: draft.profile_id, model: draft.model.trim() })}>{t("common.save")}</button>
      {changed && <button className="secondary-button" type="button" onClick={() => setDraft(value)}>{t("common.cancel")}</button>}
    </div>
  </>;
}

export function ModelBindingEditor({ value, profiles, capability, disabled = false, hideBinding = false, floatingLayer = "page", onChange }: {
  value: ModelChoice;
  profiles: ApiProfileView[];
  capability: ApiCapability;
  disabled?: boolean;
  hideBinding?: boolean;
  floatingLayer?: "page" | "dialog";
  onChange: (value: ModelChoice) => void;
}) {
  const { t } = useTranslation();
  const compatible = profiles.filter((profile) => profileSupportsCapability(profile, capability));
  const selected = compatible.find((profile) => profile.id === value.profile_id);
  const usesModels = capability !== "text_translation" || Boolean(selected?.capabilities.supports_model_listing);
  const catalog = useProfileModels(selected, usesModels && !disabled);
  const models = capability === "image_understanding"
    ? visionModels(selected?.provider, { models: catalog.models, model_details: catalog.modelDetails })
    : translationModels(catalog.models, catalog.modelDetails);
  const unavailable = Boolean(value.profile_id && !selected);
  return <div className="model-binding-editor">
    {!hideBinding && <div className="field">
      <span>{t("settings.translation.profile")}</span>
      <DropdownField label={t("settings.translation.profile")} value={value.profile_id ?? ""}
        floating floatingLayer={floatingLayer} searchable disabled={disabled}
        options={[
          { value: "", label: t("settings.modelAssignments.selectProfile") },
          ...compatible.map((profile) => ({ value: profile.id, label: profile.name, description: profile.provider_display_name })),
          ...(unavailable ? [{ value: value.profile_id!, label: modelBindingLabel(value, profiles, ""),
            description: t("settings.modelAssignments.unavailable") }] : []),
        ]}
        onChange={(profile_id) => {
          const next = compatible.find((profile) => profile.id === profile_id);
          onChange("target_language" in value && next
            ? { ...switchTranslationProfile(value as TranslationTargetSettings, next), reasoning_control: null }
            : { ...value, profile_id: profile_id || null, model: "",
              ...("thinking_enabled" in value ? { reasoning_control: null } : {}) });
        }} />
    </div>}
    {!hideBinding && usesModels && <div className="field">
      <span>{t("settings.translation.model")}</span>
      <div className="translation-model-input-row">
        <EditableDropdownField label={t("settings.translation.model")} value={value.model}
          floating floatingLayer={floatingLayer} commitOnBlur disabled={disabled || !selected}
          placeholder={t("settings.translation.manualModel")}
          optionsDisabled={catalog.loading}
          options={models.map((model) => ({ value: model, label: model }))}
          onChange={(model) => onChange({ ...("target_language" in value
            ? updateTranslationModel(value as TranslationTargetSettings, model.trim()) : { ...value, model: model.trim() }),
            ...("thinking_enabled" in value ? { reasoning_control: normalizeReasoningControlForModel(
              selected?.provider, model.trim(), value.reasoning_control, catalog.modelDetails[model.trim()],
            ) } : {}) })} />
        <button className="api-row-icon-button" type="button"
          aria-label={t("settings.apiManagement.refreshModels")} data-tooltip={t("settings.apiManagement.refreshModels")}
          disabled={disabled || !selected || catalog.loading} onClick={() => void catalog.refresh()}>
          <RefreshCw size={15} className={catalog.loading ? "spin" : undefined} />
        </button>
      </div>
    </div>}
    {unavailable && <small className="api-model-catalog-error" role="status">{t("settings.modelAssignments.unavailable")}</small>}
    {selected && selected.capabilities.requires_api_key && !selected.credential.configured
      && <small className="api-model-catalog-error">{t("settings.apiManagement.validation.apiKey")}</small>}
    {catalog.error && <small className="api-model-catalog-error" role="alert">{catalog.error}</small>}
    {usesModels && selected && !catalog.loading && !catalog.error && !models.length
      && <small>{t("settings.apiManagement.modelsUnavailable")}</small>}
    {"thinking_enabled" in value && <ReasoningControlField provider={selected?.provider} model={value.model}
      details={catalog.modelDetails[value.model]} thinkingEnabled={value.thinking_enabled ?? false}
      value={value.reasoning_control} disabled={disabled || !selected}
      onChange={(patch) => onChange({ ...value, ...patch })} />}
  </div>;
}

export function ModelSelectionField({ label, dialogLabel, icon, code, compact = false, value, fallback, profiles, capability, disabled = false, onChange }: {
  label: string;
  dialogLabel?: string;
  icon?: ReactNode;
  code?: string;
  compact?: boolean;
  value: ModelChoice;
  fallback: ModelBinding;
  profiles: ApiProfileView[];
  capability: ApiCapability;
  disabled?: boolean;
  onChange: (value: ModelChoice) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState(value);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const resolved = resolveModelSelection(value, fallback);
  const profile = profiles.find((item) => item.id === resolved.profile_id);
  const unavailable = Boolean(resolved.profile_id && (!profile || !profileSupportsCapability(profile, capability)
    || (profile.status && profile.status !== "ready")));
  const draftResolved = resolveModelSelection(draft, fallback);
  const draftProfile = profiles.find((item) => item.id === draftResolved.profile_id);
  const modelRequired = capability !== "text_translation" || draftProfile?.capabilities.supports_model_listing;
  const canSave = Boolean(draftProfile && profileSupportsCapability(draftProfile, capability)
    && (!modelRequired || draftResolved.model.trim()));
  const changeSource = (source: string) => setDraft({ ...draft, ...draftResolved, use_default_model: source === "default" });
  const targetTitle = dialogLabel ?? label;
  return <div className={`model-selection-field${compact ? " compact" : ""}`}>
    <div className="model-selection-copy">
      <div className="model-selection-identity">
        {icon && <span className={`model-selection-icon${compact ? " compact" : ""}`} aria-hidden="true">{icon}</span>}
        <div className="model-selection-title">
          <strong>{label}</strong>
          {code && <code className="model-selection-code">{code}</code>}
        </div>
      </div>
      <div className="model-selection-summary">
        <span className={unavailable ? "api-model-catalog-error" : !resolved.profile_id ? "model-selection-unconfigured" : undefined}>
          {modelBindingLabel(resolved, profiles, t("settings.apiManagement.notConfigured"))}
        </span>
        <div className="model-selection-badges">
          <small className={`model-selection-badge ${value.use_default_model ? "default" : "independent"}`}>
            {t(`settings.modelAssignments.${value.use_default_model ? "followingDefault" : "independent"}`)}
          </small>
          {unavailable && <small className="model-selection-badge error">{t("settings.modelAssignments.unavailable")}</small>}
        </div>
      </div>
    </div>
    <button ref={triggerRef} className="secondary-button model-selection-change" type="button" disabled={disabled}
      aria-label={`${t("common.edit")} · ${targetTitle}`} data-tooltip={t("settings.modelAssignments.changeModel")}
      onClick={() => { setDraft(value); setOpen(true); }}><Pencil size={14} /><span>{t("common.edit")}</span></button>
    {open && <SettingsDialog label={targetTitle} saving={false} returnFocusRef={triggerRef} autoFocus
      className="model-selection-dialog" onClose={() => setOpen(false)}>
      <div className="model-selection-dialog-shell">
        <div className="model-selection-dialog-heading">
          <strong>{targetTitle}</strong>
        </div>
        <div className="model-selection-dialog-body" data-floating-boundary>
          <div className="field">
            <span>{t("settings.modelAssignments.source")}</span>
            <DropdownField label={t("settings.modelAssignments.source")} value={draft.use_default_model ? "default" : "custom"}
              floating floatingLayer="dialog" options={[
                { value: "default", label: t("settings.modelAssignments.followingDefault") },
                { value: "custom", label: t("settings.modelAssignments.independent") },
              ]} onChange={changeSource} />
          </div>
          {draft.use_default_model && <div className="model-selection-inherited-card">
            <span>{t("settings.modelAssignments.followingDefault")}</span>
            <strong>{modelBindingLabel(fallback, profiles, t("settings.apiManagement.notConfigured"))}</strong>
          </div>}
          {(!draft.use_default_model || "thinking_enabled" in draft) && <ModelBindingEditor value={draftResolved}
            profiles={profiles} capability={capability} hideBinding={draft.use_default_model}
            floatingLayer="dialog" onChange={setDraft} />}
        </div>
        <div className="model-default-actions model-selection-dialog-actions">
          <button className="secondary-button" type="button" onClick={() => setOpen(false)}>{t("common.cancel")}</button>
          <button className="primary-button" type="button"
            disabled={!canSave}
            onClick={() => { onChange(draftResolved); setOpen(false); }}>{t("common.save")}</button>
        </div>
      </div>
    </SettingsDialog>}
  </div>;
}
