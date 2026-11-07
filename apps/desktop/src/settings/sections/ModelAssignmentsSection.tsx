import { useRef, useState } from "react";
import { Eye, FileText, Languages, Layers2, Link2, MessageSquareText, ScanText, Sparkles } from "lucide-react";
import { useTranslation } from "react-i18next";

import { ConfirmDialog } from "../../app/components/ConfirmDialog";
import { profileSupportsCapability } from "../../providers/provider-catalog";
import type { ApiProfileView } from "../../providers/types";
import { DefaultModelEditor, ModelSelectionField } from "../api/ModelSelectionField";
import {
  applyModelDefaults,
  DEFAULT_LEARNING_AI,
  DEFAULT_MODEL_SELECTION,
  EMPTY_MODEL_BINDING,
  translationSelectionReady,
} from "../model-selection";
import type { ApplySettings } from "../settings-types";
import type { Settings } from "../types";

export function ModelAssignmentsSection({ settings, profiles, applySettings }: {
  settings: Settings;
  profiles: ApiProfileView[];
  applySettings: ApplySettings;
}) {
  const { t } = useTranslation();
  const [confirmBatch, setConfirmBatch] = useState(false);
  const batchRef = useRef<HTMLButtonElement>(null);
  const defaults = {
    text: settings.model_defaults?.text ?? EMPTY_MODEL_BINDING,
    translation: settings.model_defaults?.translation ?? settings.model_defaults?.text ?? EMPTY_MODEL_BINDING,
    vision: settings.model_defaults?.vision ?? EMPTY_MODEL_BINDING,
  };
  const update: ApplySettings = (change) => applySettings((current) => applyModelDefaults(change(current)));
  const textProfile = profiles.find((profile) => profile.id === defaults.text.profile_id);
  const visionProfile = profiles.find((profile) => profile.id === defaults.vision.profile_id);
  const textReady = Boolean(defaults.text.model.trim() && textProfile && profileSupportsCapability(textProfile, "text_generation"));
  const translationDefaultReady = translationSelectionReady(defaults.translation, profiles);
  const visionReady = Boolean(defaults.vision.model.trim() && visionProfile && profileSupportsCapability(visionProfile, "image_understanding"));
  const batchTasks = [
    translationDefaultReady && t("settings.modelAssignments.translation"), translationDefaultReady && t("settings.modelAssignments.ocrTranslation"),
    textReady && t("settings.modelAssignments.conversationTitles"), textReady && t("settings.modelAssignments.learningAi"),
    visionReady && t("settings.modelAssignments.visionQuestions"),
  ].filter(Boolean).join(", ");
  const followDefaults = () => update((current) => ({
    ...current,
    translation: translationDefaultReady ? { ...current.translation, model_selection: { ...DEFAULT_MODEL_SELECTION },
    } : current.translation,
    ocr: translationDefaultReady ? { ...current.ocr, model_selection: { ...DEFAULT_MODEL_SELECTION } } : current.ocr,
    conversation_titles: textReady ? { ...current.conversation_titles, use_default_model: true } : current.conversation_titles,
    vision: visionReady ? { ...current.vision, use_default_model: true } : current.vision,
    learning_ai: textReady ? { ...(current.learning_ai ?? DEFAULT_LEARNING_AI), use_default_model: true } : current.learning_ai,
  }));
  return <div className="model-assignments-section settings-section-active">
    <div className="model-defaults-grid">
      <DefaultModelEditor title={t("settings.modelAssignments.textDefault")}
        icon={<FileText size={17} />} value={defaults.text} profiles={profiles}
        capability="text_generation" onChange={(value) => update((current) => ({
          ...current, model_defaults: { ...current.model_defaults, text: value },
        }))} />
      <DefaultModelEditor title={t("settings.modelAssignments.translationDefault")}
        icon={<Languages size={17} />} value={defaults.translation} profiles={profiles}
        capability="text_translation" onChange={(value) => update((current) => ({
          ...current, model_defaults: { ...current.model_defaults, translation: value },
        }))} />
      <DefaultModelEditor title={t("settings.modelAssignments.visionDefault")}
        icon={<Eye size={17} />} value={defaults.vision} profiles={profiles}
        capability="image_understanding" onChange={(value) => update((current) => ({
          ...current, model_defaults: { ...current.model_defaults, vision: value },
        }))} />
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
            fallback={defaults.translation} profiles={profiles} capability="text_translation"
            onChange={(value) => update((current) => ({ ...current, translation: { ...current.translation, model_selection: value } }))} />
        </div>
        <div className="model-assignment-item">
          <ModelSelectionField icon={<ScanText size={16} />} label={t("settings.modelAssignments.ocrTranslation")}
            value={settings.ocr.model_selection ?? DEFAULT_MODEL_SELECTION}
            fallback={defaults.translation} profiles={profiles} capability="text_translation"
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
