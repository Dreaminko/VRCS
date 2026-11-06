import { useRef, useState } from "react";
import { Eye, FileText, Info, Languages, Layers2, Link2, MessageSquareText, ScanText, Sparkles } from "lucide-react";
import { useTranslation } from "react-i18next";

import { ConfirmDialog } from "../../app/components/ConfirmDialog";
import { liveTranslationServiceName } from "../../providers/recognition-services";
import { profileSupportsCapability } from "../../providers/provider-catalog";
import type { ApiProfileView } from "../../providers/types";
import { localizedLanguageName } from "../../translation/translation-languages";
import { DefaultModelEditor, ModelSelectionField } from "../api/ModelSelectionField";
import { applyModelDefaults, DEFAULT_LEARNING_AI, DEFAULT_MODEL_SELECTION, EMPTY_MODEL_BINDING, translationModelSelection } from "../model-selection";
import type { ApplySettings } from "../settings-types";
import type { Settings, TranslationTargetSettings } from "../types";

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
  const translationReady = Boolean(textReady && textProfile && profileSupportsCapability(textProfile, "text_translation"));
  const visionReady = Boolean(defaults.vision.model.trim() && visionProfile && profileSupportsCapability(visionProfile, "image_understanding"));
  const batchTasks = [
    translationReady && t("settings.modelAssignments.translation"), translationReady && t("settings.modelAssignments.ocrTranslation"),
    textReady && t("settings.modelAssignments.conversationTitles"), textReady && t("settings.modelAssignments.learningAi"),
    visionReady && t("settings.modelAssignments.visionQuestions"),
  ].filter(Boolean).join(", ");
  const inheritTargets = (targets: TranslationTargetSettings[]) => targets.map((target) => ({ ...target, use_default_model: true }));
  const followDefaults = () => update((current) => ({
    ...current,
    translation: translationReady ? { ...current.translation, model_selection: { ...DEFAULT_MODEL_SELECTION },
      speaker_targets: inheritTargets(current.translation.speaker_targets),
      microphone_targets: inheritTargets(current.translation.microphone_targets) } : current.translation,
    ocr: translationReady ? { ...current.ocr, model_selection: { ...DEFAULT_MODEL_SELECTION }, targets: inheritTargets(current.ocr.targets) } : current.ocr,
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
          return <ModelSelectionField key={`${index}-${target.target_language}`}
            compact
            icon={<span className="model-route-rank">{index + 1}</span>}
            label={languageName}
            code={languageName.toLowerCase() !== target.target_language.toLowerCase() ? target.target_language : undefined}
            dialogLabel={`${scope} · ${languageName}`}
            value={target} fallback={translationModelSelection(settings, ocr)}
            profiles={profiles} capability="text_translation" onChange={(value) => update((current) => {
              const patch = (targets: TranslationTargetSettings[]) => targets.map((item, i) => i === index ? { ...item, ...value } : item);
              return key === "ocr" ? { ...current, ocr: { ...current.ocr, targets: patch(current.ocr.targets) } }
                : { ...current, translation: { ...current.translation, [key]: patch(current.translation[key]) } };
            })} />;
        })}
      </div>
    </div>;
  };
  const hasTranslationSubroutes = Boolean(
    nativeTranslation
    || settings.translation.microphone_targets.length > 0
    || settings.translation.speaker_targets.length > 0,
  );
  return <div className="model-assignments-section">
    <div className="model-defaults-grid">
      {(["text", "vision"] as const).map((kind) => <section className="system-settings-group model-default-card" key={kind}>
        <div className="section-heading model-default-heading">
          <div>
            {kind === "vision" ? <Eye size={17} /> : <FileText size={17} />}
            <h3>{t(`settings.modelAssignments.${kind}Default`)}</h3>
          </div>
        </div>
        <DefaultModelEditor value={defaults[kind]} profiles={profiles}
          capability={kind === "vision" ? "image_understanding" : "text_generation"}
          onChange={(value) => update((current) => ({ ...current, model_defaults: {
            ...current.model_defaults, [kind]: { profile_id: value.profile_id, model: value.model },
          } }))} />
      </section>)}
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
            fallback={defaults.text} profiles={profiles} capability="text_translation"
            onChange={(value) => update((current) => ({ ...current, translation: { ...current.translation, model_selection: value } }))} />
          {hasTranslationSubroutes && <div className="model-assignment-subroutes">
            {nativeTranslation && <p className="settings-field-note model-assignment-note">
              <Info size={14} aria-hidden="true" />
              <span>{t("settings.modelAssignments.nativeTranslation", { service: nativeTranslation })}</span>
            </p>}
            {routes("microphone_targets")}
            {routes("speaker_targets")}
          </div>}
        </div>
        <div className="model-assignment-item">
          <ModelSelectionField icon={<ScanText size={16} />} label={t("settings.modelAssignments.ocrTranslation")}
            value={settings.ocr.model_selection ?? DEFAULT_MODEL_SELECTION}
            fallback={defaults.text} profiles={profiles} capability="text_translation"
            onChange={(value) => update((current) => ({ ...current, ocr: { ...current.ocr, model_selection: value } }))} />
          {settings.ocr.targets.length > 0 && <div className="model-assignment-subroutes">
            {routes("ocr")}
          </div>}
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
