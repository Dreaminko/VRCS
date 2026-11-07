import { useRef, useState } from "react";
import { Eye, FileText, Info, Languages, Layers2, Link2, MessageSquareText, ScanText, Sparkles } from "lucide-react";
import { useTranslation } from "react-i18next";

import { ConfirmDialog } from "../../app/components/ConfirmDialog";
import { liveTranslationServiceName } from "../../providers/recognition-services";
import { profileSupportsCapability } from "../../providers/provider-catalog";
import type { ApiProfileView } from "../../providers/types";
import { localizedLanguageName } from "../../translation/translation-languages";
import { DefaultModelEditor, ModelBindingEditor, ModelSelectionField, ModelSourceToggle, modelBindingLabel } from "../api/ModelSelectionField";
import {
  applyModelDefaults,
  DEFAULT_LEARNING_AI,
  DEFAULT_MODEL_SELECTION,
  EMPTY_MODEL_BINDING,
  resolveModelSelection,
  translationModelSelection,
  translationSelectionReady,
} from "../model-selection";
import type { ApplySettings } from "../settings-types";
import type { ModelBinding, Settings, TranslationTargetSettings } from "../types";

function TargetModelRow({ index, label, code, target, fallback, profiles, onChange }: {
  index: number;
  label: string;
  code?: string;
  target: TranslationTargetSettings;
  fallback: ModelBinding;
  profiles: ApiProfileView[];
  onChange: (target: TranslationTargetSettings) => void;
}) {
  const { t } = useTranslation();
  const resolved = resolveModelSelection(target, fallback);
  const usesDefault = Boolean(target.use_default_model);
  return <div className="model-target-route">
    <div className="model-target-route-heading">
      <span className="model-target-route-rank">{index + 1}</span>
      <div className="model-target-route-title">
        <strong>{label}</strong>
        {code && <code className="model-selection-code">{code}</code>}
      </div>
      <ModelSourceToggle useDefaultModel={usesDefault} label={`${t("settings.modelAssignments.source")} · ${label}`}
        onChange={(useDefaultModel) => onChange({
          ...target, ...resolved, use_default_model: useDefaultModel,
        })} />
    </div>
    {usesDefault
      ? <div className="model-target-route-inherited">
        <span>{t("settings.modelAssignments.followingDefault")}</span>
        <strong>{modelBindingLabel(fallback, profiles, t("settings.apiManagement.notConfigured"))}</strong>
      </div>
      : <ModelBindingEditor value={resolved} profiles={profiles} capability="text_translation" floatingLayer="dialog"
        onChange={(value) => onChange({ ...target, ...value, use_default_model: false })} />}
  </div>;
}

export function ModelAssignmentsSection({ settings, profiles, applySettings }: {
  settings: Settings;
  profiles: ApiProfileView[];
  applySettings: ApplySettings;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const [confirmBatch, setConfirmBatch] = useState(false);
  const batchRef = useRef<HTMLButtonElement>(null);
  const defaults = settings.model_defaults ?? { text: EMPTY_MODEL_BINDING, vision: EMPTY_MODEL_BINDING };
  const update: ApplySettings = (change) => applySettings((current) => applyModelDefaults(change(current)));
  const nativeTranslation = settings.translation.mode === "automatic" && liveTranslationServiceName(settings.asr.backend);
  const textProfile = profiles.find((profile) => profile.id === defaults.text.profile_id);
  const visionProfile = profiles.find((profile) => profile.id === defaults.vision.profile_id);
  const textReady = Boolean(defaults.text.model.trim() && textProfile && profileSupportsCapability(textProfile, "text_generation"));
  // Batch defaults only apply when the shared API text model can also translate.
  // An independently selected local task remains valid even when it cannot follow that default.
  const translationDefaultReady = translationSelectionReady(defaults.text, profiles);
  const visionReady = Boolean(defaults.vision.model.trim() && visionProfile && profileSupportsCapability(visionProfile, "image_understanding"));
  const batchTasks = [
    translationDefaultReady && t("settings.modelAssignments.translation"), translationDefaultReady && t("settings.modelAssignments.ocrTranslation"),
    textReady && t("settings.modelAssignments.conversationTitles"), textReady && t("settings.modelAssignments.learningAi"),
    visionReady && t("settings.modelAssignments.visionQuestions"),
  ].filter(Boolean).join(", ");
  const inheritTargets = (targets: TranslationTargetSettings[]) => targets.map((target) => ({ ...target, use_default_model: true }));
  const followDefaults = () => update((current) => ({
    ...current,
    translation: translationDefaultReady ? { ...current.translation, model_selection: { ...DEFAULT_MODEL_SELECTION },
      speaker_targets: inheritTargets(current.translation.speaker_targets),
      microphone_targets: inheritTargets(current.translation.microphone_targets) } : current.translation,
    ocr: translationDefaultReady ? { ...current.ocr, model_selection: { ...DEFAULT_MODEL_SELECTION }, targets: inheritTargets(current.ocr.targets) } : current.ocr,
    conversation_titles: textReady ? { ...current.conversation_titles, use_default_model: true } : current.conversation_titles,
    vision: visionReady ? { ...current.vision, use_default_model: true } : current.vision,
    learning_ai: textReady ? { ...(current.learning_ai ?? DEFAULT_LEARNING_AI), use_default_model: true } : current.learning_ai,
  }));
  const routes = (key: "microphone_targets" | "speaker_targets" | "ocr") => {
    const ocr = key === "ocr";
    const items = ocr ? settings.ocr.targets : settings.translation[key];
    if (!items.length) return null;
    const scope = t(ocr ? "settings.modelAssignments.ocrTargets" : key === "microphone_targets"
      ? "settings.translation.targetLanguageForSelf" : "settings.translation.targetLanguageForOtherParty");
    const fallback = translationModelSelection(settings, ocr);
    const overrideCount = items.filter((target) => !target.use_default_model).length;
    return <div className="model-route-group">
      <div className="model-route-group-header">
        <span>{scope}</span>
        <span className={`model-route-count${overrideCount > 0 ? " active" : ""}`}>{t("settings.modelAssignments.routeOverrides", {
          count: overrideCount,
        })}</span>
      </div>
      <div className="model-route-list">
        {items.map((target, index) => {
          const languageName = localizedLanguageName(target.target_language, locale);
          return <TargetModelRow key={`${index}-${target.target_language}`}
            index={index}
            label={languageName}
            code={languageName.toLowerCase() !== target.target_language.toLowerCase() ? target.target_language : undefined}
            target={target} fallback={fallback} profiles={profiles}
            onChange={(value) => update((current) => {
              const patch = (targets: TranslationTargetSettings[]) => targets.map((item, i) => i === index ? { ...item, ...value } : item);
              return key === "ocr" ? { ...current, ocr: { ...current.ocr, targets: patch(current.ocr.targets) } }
                : { ...current, translation: { ...current.translation, [key]: patch(current.translation[key]) } };
            })} />;
        })}
      </div>
    </div>;
  };
  const translationDialogContent = <section className="model-selection-dialog-routes">
    <div className="model-selection-dialog-routes-heading">
      <strong>{t("settings.modelAssignments.targetLanguageModels")}</strong>
      <small>{t("settings.modelAssignments.targetLanguageModelsHint")}</small>
    </div>
    {nativeTranslation && <p className="settings-field-note model-assignment-note">
      <Info size={14} aria-hidden="true" />
      <span>{t("settings.modelAssignments.nativeTranslation", { service: nativeTranslation })}</span>
    </p>}
    {routes("microphone_targets")}
    {routes("speaker_targets")}
  </section>;
  const ocrDialogContent = settings.ocr.targets.length > 0
    ? <section className="model-selection-dialog-routes">
      <div className="model-selection-dialog-routes-heading">
        <strong>{t("settings.modelAssignments.targetLanguageModels")}</strong>
        <small>{t("settings.modelAssignments.targetLanguageModelsHint")}</small>
      </div>
      {routes("ocr")}
    </section>
    : undefined;
  return <div className="model-assignments-section settings-section-active">
    <div className="model-defaults-grid">
      {(["text", "vision"] as const).map((kind) => <DefaultModelEditor key={kind}
        title={t(`settings.modelAssignments.${kind}Default`)}
        icon={kind === "vision" ? <Eye size={17} /> : <FileText size={17} />}
        value={defaults[kind]} profiles={profiles}
        capability={kind === "vision" ? "image_understanding" : "text_generation"}
        onChange={(value) => update((current) => ({ ...current, model_defaults: {
          ...current.model_defaults, [kind]: { profile_id: value.profile_id, model: value.model },
        } }))} />)}
    </div>
    <section className="system-settings-group model-assignments-group">
      <div className="section-heading model-assignments-heading">
        <div><Layers2 size={18} /><h3>{t("settings.modelAssignments.tasks")}</h3></div>
        <button ref={batchRef} className="secondary-button" type="button" disabled={!batchTasks} onClick={() => setConfirmBatch(true)}>
          <Link2 size={14} />{t("settings.modelAssignments.useDefaults")}
        </button>
      </div>
      <div className="model-assignment-list">
        <div className="model-assignment-item">
          <ModelSelectionField icon={<Languages size={16} />} label={t("settings.modelAssignments.translation")}
            value={settings.translation.model_selection ?? DEFAULT_MODEL_SELECTION}
            fallback={defaults.text} profiles={profiles} capability="text_translation" dialogContent={translationDialogContent}
            onChange={(value) => update((current) => ({ ...current, translation: { ...current.translation, model_selection: value } }))} />
        </div>
        <div className="model-assignment-item">
          <ModelSelectionField icon={<ScanText size={16} />} label={t("settings.modelAssignments.ocrTranslation")}
            value={settings.ocr.model_selection ?? DEFAULT_MODEL_SELECTION}
            fallback={defaults.text} profiles={profiles} capability="text_translation" dialogContent={ocrDialogContent}
            onChange={(value) => update((current) => ({ ...current, ocr: { ...current.ocr, model_selection: value } }))} />
        </div>
        <div className="model-assignment-item">
          <ModelSelectionField icon={<MessageSquareText size={16} />} label={t("settings.modelAssignments.conversationTitles")}
            value={settings.conversation_titles}
            fallback={defaults.text} profiles={profiles} capability="text_generation"
            onChange={(value) => update((current) => ({ ...current, conversation_titles: { ...current.conversation_titles, ...value } }))} />
        </div>
        <div className="model-assignment-item">
          <ModelSelectionField icon={<Sparkles size={16} />} label={t("settings.modelAssignments.learningAi")}
            value={settings.learning_ai ?? DEFAULT_LEARNING_AI}
            fallback={defaults.text} profiles={profiles} capability="text_generation"
            onChange={(value) => update((current) => ({ ...current, learning_ai: { ...(current.learning_ai ?? DEFAULT_LEARNING_AI), ...value } }))} />
        </div>
        <div className="model-assignment-item">
          <ModelSelectionField icon={<Eye size={16} />} label={t("settings.modelAssignments.visionQuestions")}
            value={settings.vision}
            fallback={defaults.vision} profiles={profiles} capability="image_understanding"
            onChange={(value) => update((current) => ({ ...current, vision: { ...current.vision, ...value } }))} />
        </div>
      </div>
    </section>
    {confirmBatch && <ConfirmDialog title={t("settings.modelAssignments.useDefaults")}
      message={t("settings.modelAssignments.confirmDefaults", { tasks: batchTasks })} confirmLabel={t("settings.modelAssignments.useDefaults")}
      busy={false} returnFocusRef={batchRef} onClose={() => setConfirmBatch(false)}
      onConfirm={() => { followDefaults(); setConfirmBatch(false); }} />}
  </div>;
}
