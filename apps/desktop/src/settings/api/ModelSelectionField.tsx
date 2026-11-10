import { useEffect, useRef, useState, type ReactNode } from "react";
import { Cloud, Cpu, Pencil, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";

import { profileSupportsCapability } from "../../providers/provider-catalog";
import type { ApiCapability, ApiProfileView } from "../../providers/types";
import { DropdownField, EditableDropdownField } from "../../shared/ui/DropdownField";
import {
  PLAMO_LOCAL_MODEL_ID,
} from "../../translation/translation-languages";
import {
  switchTranslationBackend,
  switchTranslationProfile,
  selectTranslationModel,
  translationModels,
  updateTranslationModel,
} from "../../translation/translation-model-selection";
import { normalizeReasoningControlForModel } from "../../translation/translation-thinking";
import type { ReasoningSetting } from "../../translation/translation-thinking";
import { SettingsDialog } from "../components/SettingsDialog";
import { useProfileModels } from "../hooks/useProfileModels";
import { resolveModelSelection, translationSelectionReady } from "../model-selection";
import { ReasoningControlField } from "../ReasoningControlField";
import type { ModelBinding, ModelSelection, TranslationTargetSettings } from "../types";
import { visionModels } from "../vision-settings";
import { CapabilityNotice, usePlatformCapabilities } from "../../core-client/capabilities";

export interface ModelChoice extends ModelSelection {
  thinking_enabled?: boolean;
  reasoning_control?: ReasoningSetting | null;
}

export function modelBindingLabel(value: ModelBinding, profiles: ApiProfileView[], empty: string): string {
  if (value.backend === "plamo_local") return "PLaMo 3 Translate 0.8B";
  if (!value.profile_id) return empty;
  const profile = profiles.find((item) => item.id === value.profile_id);
  return [profile?.name ?? value.profile_id, value.model].filter(Boolean).join(" / ");
}

export function ModelSourceToggle({ useDefaultModel, label, onChange }: {
  useDefaultModel: boolean;
  label: string;
  onChange: (useDefaultModel: boolean) => void;
}) {
  const { t } = useTranslation();
  return <div className="theme-toggle-group model-source-toggle" role="group" aria-label={label}>
    {([
      [true, t("settings.modelAssignments.followingDefault")],
      [false, t("settings.modelAssignments.independent")],
    ] as const).map(([value, text]) => <button key={String(value)} type="button"
      aria-pressed={useDefaultModel === value} className={useDefaultModel === value ? "active" : ""}
      onClick={() => onChange(value)}>{text}</button>)}
  </div>;
}

function TranslationBackendToggle({ value, profiles, disabled = false, iconOnly = false, onChange }: {
  value: ModelChoice;
  profiles: ApiProfileView[];
  disabled?: boolean;
  iconOnly?: boolean;
  onChange: (value: ModelChoice) => void;
}) {
  const { t } = useTranslation();
  const { core } = usePlatformCapabilities();
  const apiBinding = useRef<ModelBinding | null>(
    (value.backend ?? "api") === "api"
      ? { backend: "api", profile_id: value.profile_id, model: value.model }
      : null,
  );
  const compatible = profiles.filter((profile) => profileSupportsCapability(profile, "text_translation"));
  return <div className={`theme-toggle-group translation-backend-toggle${iconOnly ? " icon-only" : ""}`}
    role="group" aria-label={t("settings.translation.backend")}>
    {(["api", "plamo_local"] as const).map((backend) => {
      const label = t(`settings.translation.backends.${backend}`);
      return <button key={backend} type="button" aria-label={label}
        aria-pressed={(value.backend ?? "api") === backend}
        className={(value.backend ?? "api") === backend ? "active" : ""}
        disabled={disabled || (backend === "plamo_local" && !core?.inference.local_translation.supported)}
        title={backend === "plamo_local" && !core?.inference.local_translation.supported
          ? t("platform.local_translation_unavailable") : undefined}
        data-tooltip={iconOnly ? label : undefined}
        onClick={() => {
          if ((value.backend ?? "api") === backend) return;
          if (backend === "plamo_local") {
            apiBinding.current = {
              backend: "api",
              profile_id: value.profile_id,
              model: value.model,
            };
            onChange(switchTranslationBackend(value, backend));
          } else {
            onChange({
              ...value,
              ...(apiBinding.current ?? {
                backend: "api",
                profile_id: compatible[0]?.id ?? null,
                model: "",
              }),
              backend: "api",
            });
          }
        }}>
        {iconOnly ? (backend === "api" ? <Cloud size={14} /> : <Cpu size={14} />) : label}
      </button>;
    })}
  </div>;
}

export function DefaultModelEditor({ title, icon, value, profiles, capability, onChange }: {
  title: string;
  icon?: ReactNode;
  value: ModelBinding;
  profiles: ApiProfileView[];
  capability: ApiCapability;
  onChange: (value: ModelBinding) => void;
}) {
  const translation = capability === "text_translation";
  const [pendingApi, setPendingApi] = useState<ModelBinding | null>(null);
  const displayed = translation ? pendingApi ?? value : value;
  const apply = (next: ModelChoice) => {
    const binding = {
      backend: next.backend ?? "api",
      profile_id: next.profile_id,
      model: next.model.trim(),
    } satisfies ModelBinding;
    if (translation && binding.backend === "api" && !translationSelectionReady(binding, profiles)) {
      setPendingApi(binding);
      return;
    }
    setPendingApi(null);
    onChange(binding);
  };
  return <section className="system-settings-group model-default-card">
    <div className="section-heading model-default-heading">
      <div>{icon}<h3>{title}</h3>
        {translation && <TranslationBackendToggle iconOnly value={displayed} profiles={profiles} onChange={apply} />}
      </div>
    </div>
    <ModelBindingEditor value={displayed} profiles={profiles} capability={capability}
      autoSelectModel={translation} hideBackend={translation} onChange={apply} />
  </section>;
}

export function ModelBindingEditor({
  value,
  profiles,
  capability,
  disabled = false,
  hideBinding = false,
  hideBackend = false,
  autoSelectModel = false,
  floatingLayer = "page",
  onChange,
}: {
  value: ModelChoice;
  profiles: ApiProfileView[];
  capability: ApiCapability;
  disabled?: boolean;
  hideBinding?: boolean;
  hideBackend?: boolean;
  autoSelectModel?: boolean;
  floatingLayer?: "page" | "dialog";
  onChange: (value: ModelChoice) => void;
}) {
  const { t } = useTranslation();
  const { core } = usePlatformCapabilities();
  const local = capability === "text_translation" && value.backend === "plamo_local";
  const compatible = profiles.filter((profile) => profileSupportsCapability(profile, capability));
  const selected = compatible.find((profile) => profile.id === value.profile_id);
  const usesModels = capability !== "text_translation" || Boolean(selected?.capabilities.supports_model_listing);
  const catalog = useProfileModels(selected, usesModels && !disabled);
  const models = capability === "image_understanding"
    ? visionModels(selected?.provider, { models: catalog.models, model_details: catalog.modelDetails })
    : translationModels(catalog.models, catalog.modelDetails);
  const unavailable = Boolean(value.profile_id && !selected);
  useEffect(() => {
    if (!autoSelectModel || local || !selected?.capabilities.supports_model_listing
      || value.model.trim() || catalog.loading || catalog.error || !models.length) return;
    const model = selectTranslationModel(selected.provider, models, "", catalog.modelDetails);
    if (model) onChange({ ...value, model });
  }, [
    autoSelectModel,
    catalog.error,
    catalog.loading,
    catalog.modelDetails,
    local,
    models,
    onChange,
    selected,
    value,
  ]);
  return <div className="model-binding-editor" data-backend={value.backend ?? "api"}>
    {!hideBinding && !hideBackend && capability === "text_translation" && <div className="field model-backend-field">
      <span>{t("settings.translation.backend")}</span>
      <TranslationBackendToggle iconOnly value={value} profiles={profiles} disabled={disabled} onChange={onChange} />
    </div>}
    {local && <div className="model-selection-inherited-card" role="status">
      <span>{t("settings.translation.localModel")}</span>
      <strong>PLaMo 3 Translate 0.8B · CPU</strong>
    </div>}
    {local && <CapabilityNotice capability={core?.inference.local_translation} />}
    {!local && !hideBinding && <div className="field">
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
    {!local && !hideBinding && usesModels && <div className="field">
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
    {!local && unavailable && <small className="api-model-catalog-error" role="status">{t("settings.modelAssignments.unavailable")}</small>}
    {!local && selected && selected.capabilities.requires_api_key && !selected.credential.configured
      && <small className="api-model-catalog-error">{t("settings.apiManagement.validation.apiKey")}</small>}
    {!local && catalog.error && <small className="api-model-catalog-error" role="alert">{catalog.error}</small>}
    {!local && usesModels && selected && !catalog.loading && !catalog.error && !models.length
      && <small>{t("settings.apiManagement.modelsUnavailable")}</small>}
    {!local && "thinking_enabled" in value && <ReasoningControlField provider={selected?.provider} model={value.model}
      details={catalog.modelDetails[value.model]} thinkingEnabled={value.thinking_enabled ?? false}
      value={value.reasoning_control} disabled={disabled || !selected} floatingLayer={floatingLayer}
      onChange={(patch) => onChange({ ...value, ...patch })} />}
  </div>;
}

export function ModelSelectionField({ label, dialogLabel, icon, code, value, fallback, profiles, capability, disabled = false, onChange }: {
  label: string;
  dialogLabel?: string;
  icon?: ReactNode;
  code?: string;
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
  const draftLocal = capability === "text_translation" && draftResolved.backend === "plamo_local";
  const draftProfile = profiles.find((item) => item.id === draftResolved.profile_id);
  const modelRequired = !draftLocal && (capability !== "text_translation" || draftProfile?.capabilities.supports_model_listing);
  const canSave = draftLocal || Boolean(draftProfile && profileSupportsCapability(draftProfile, capability)
    && (!modelRequired || draftResolved.model.trim()));
  const changeSource = (useDefaultModel: boolean) => setDraft({ ...draft, ...draftResolved, use_default_model: useDefaultModel });
  const targetTitle = dialogLabel ?? label;
  return <div className="model-selection-field">
    <div className="model-selection-copy">
      <div className="model-selection-identity">
        {icon && <span className="model-selection-icon" aria-hidden="true">{icon}</span>}
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
          <div className="model-source-row">
            <h3 className="model-source-heading">{t("settings.modelAssignments.source")}</h3>
            <ModelSourceToggle useDefaultModel={Boolean(draft.use_default_model)}
              label={t("settings.modelAssignments.source")} onChange={changeSource} />
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
