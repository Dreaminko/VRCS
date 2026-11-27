import { useRef } from "react";
import { ArrowUp, Plus, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import {
  ASR_LANGUAGE_CODES,
  MAX_ASR_PREFERRED_LANGUAGES,
} from "../../providers/recognition-services";
import type { RecognitionLanguageMode } from "../../providers/types";
import { useAnimatedList } from "../../shared/hooks/use-animated-list";
import { LanguagePicker } from "../../shared/ui/LanguagePicker";
import { localizedLanguageName } from "../../translation/translation-languages";
import { Select } from "../SettingsControls";

const ADD_BUTTON_KEY = "__add__";

export function RecognitionLanguageSelect({
  label,
  value,
  mode = "single",
  model,
  disabled,
  floating,
  supportedLanguages,
  onChange,
}: {
  label?: string;
  value: string[];
  mode?: RecognitionLanguageMode;
  model?: string;
  disabled: boolean;
  floating?: "page" | "dialog";
  supportedLanguages?: readonly string[];
  onChange: (value: string[]) => void;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const resolvedLabel = label ?? t("settings.recognition.language");
  const labelFor = (code: string) => localizedLanguageName(code, locale);
  const allCodes = supportedLanguages?.length
    ? ASR_LANGUAGE_CODES.filter((code) => supportedLanguages.includes(code))
    : [...ASR_LANGUAGE_CODES];

  const valueRef = useRef(value);
  const onChangeRef = useRef(onChange);
  valueRef.current = value;
  onChangeRef.current = onChange;

  const animatedList = useAnimatedList(value, mode !== "auto_only");

  if (mode === "auto_only") {
    return (
      <div className="field asr-language-field">
        <div className="asr-language-header">
          <span>{resolvedLabel}</span>
        </div>
        <Select
          label={resolvedLabel}
          hideLabel
          value="auto"
          options={[{ value: "auto", label: t("languages.auto") }]}
          disabled
          floating={floating}
          onChange={() => undefined}
        />
        {value.length > 0 && (
          <small role="note">
            {t("settings.recognition.autoOnlyPreservedHint", {
              languages: value.map(labelFor).join(" / "),
            })}
          </small>
        )}
      </div>
    );
  }

  const unsupportedPrimary = Boolean(
    supportedLanguages?.length && value.length > 0 && !supportedLanguages.includes(value[0]),
  );

  const remainingCodes = allCodes.filter((code) => !value.includes(code));
  const moveFirst = (index: number) => {
    if (index <= 0 || index >= value.length) return;
    const next = [...value];
    const [picked] = next.splice(index, 1);
    next.unshift(picked);
    onChange(next);
  };
  const removeCode = (code: string) => {
    animatedList.scheduleRemove(code, () => {
      const latest = valueRef.current;
      if (latest.includes(code)) {
        onChangeRef.current(latest.filter((item) => item !== code));
      }
    });
  };
  const clearAll = () => {
    animatedList.clearPendingRemovals();
    onChange([]);
  };

  return (
    <div className="field asr-language-field">
      <div className="asr-language-header">
        <span>{resolvedLabel}</span>
        {value.length > 0 && (
          <button
            className="asr-language-clear"
            type="button"
            disabled={disabled}
            onClick={clearAll}
          >
            {t("languages.auto")}
          </button>
        )}
      </div>
      <div className={`asr-language-box${disabled ? " is-disabled" : ""}`} ref={animatedList.bindContainerRef}>
        {value.length === 0 ? (
          <span className="asr-language-auto">{t("languages.auto")}</span>
        ) : (
          value.map((code, index) => {
            const removing = animatedList.isRemoving(code);
            return (
              <span
                className={removing ? "language-chip asr-language-chip is-removing" : "language-chip asr-language-chip"}
                key={code}
                ref={animatedList.bindItemRef(code)}
              >
                {value.length > 1 && <span className="asr-language-rank">{index + 1}</span>}
                <span>{labelFor(code)}</span>
                {index > 0 && (
                  <button
                    type="button"
                    disabled={disabled || removing}
                    aria-label={t("settings.recognition.prioritizeLanguage", {
                      language: labelFor(code),
                    })}
                    onClick={() => moveFirst(index)}
                  >
                    <ArrowUp size={12} aria-hidden="true" />
                  </button>
                )}
                <button
                  type="button"
                  disabled={disabled || removing}
                  aria-label={t("settings.recognition.removeLanguage", {
                    language: labelFor(code),
                  })}
                  onClick={() => removeCode(code)}
                >
                  <X size={12} aria-hidden="true" />
                </button>
              </span>
            );
          })
        )}
        <span className="asr-language-add" ref={animatedList.bindItemRef(ADD_BUTTON_KEY)}>
          <LanguagePicker
            label={value.length >= MAX_ASR_PREFERRED_LANGUAGES
              ? t("settings.recognition.preferredLanguagesLimit", { max: MAX_ASR_PREFERRED_LANGUAGES })
              : t("settings.recognition.addPreferredLanguage")}
            value=""
            languageCodes={remainingCodes}
            icon={<Plus size={14} aria-hidden="true" />}
            iconOnly
            compact
            floating
            disabled={disabled || value.length >= MAX_ASR_PREFERRED_LANGUAGES || remainingCodes.length === 0}
            onChange={(next) => {
              if (!next || value.includes(next) || value.length >= MAX_ASR_PREFERRED_LANGUAGES) return;
              onChange([...value, next]);
            }}
          />
        </span>
      </div>
      {mode === "single" && value.length > 1 && (
        <small role="note">
          {t("settings.recognition.singleLanguageHint", {
            language: labelFor(value[0]),
          })}
        </small>
      )}
      {mode === "preferred_list" && model?.startsWith("fun-asr-realtime") && value.length > 1 && (
        <small role="note">
          {t("settings.recognition.funAsrLegacyHint", {
            language: labelFor(value[0]),
          })}
        </small>
      )}
      {unsupportedPrimary && supportedLanguages && (
        <small role="alert">
          {t("settings.recognition.unsupportedPrimaryLanguageHint", {
            language: labelFor(value[0]),
            supported: supportedLanguages.map(labelFor).join(" / "),
          })}
        </small>
      )}
    </div>
  );
}

export function RecognitionLanguagesGroup({
  speakerLanguages,
  microphoneLanguages,
  mode = "single",
  model,
  disabled,
  floating,
  supportedLanguages,
  onChangeSpeaker,
  onChangeMicrophone,
}: {
  speakerLanguages: string[];
  microphoneLanguages: string[];
  mode?: RecognitionLanguageMode;
  model?: string;
  disabled: boolean;
  floating?: "page" | "dialog";
  supportedLanguages?: readonly string[];
  onChangeSpeaker: (value: string[]) => void;
  onChangeMicrophone: (value: string[]) => void;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const same = speakerLanguages.length === microphoneLanguages.length
    && speakerLanguages.every((code, index) => code === microphoneLanguages[index]);

  if (mode === "auto_only") {
    const labelFor = (code: string) => localizedLanguageName(code, locale);
    const formatList = (codes: string[]) => (codes.length ? codes.map(labelFor).join(" / ") : t("languages.auto"));
    const preserved = same
      ? formatList(speakerLanguages)
      : `${t("settings.recognition.speakerLanguage")}: ${formatList(speakerLanguages)} · ${t("settings.recognition.microphoneLanguage")}: ${formatList(microphoneLanguages)}`;

    return (
      <>
        <Select
          label={t("settings.recognition.language")}
          value="auto"
          options={[{ value: "auto", label: t("languages.auto") }]}
          disabled
          floating={floating}
          onChange={() => undefined}
        />
        {(speakerLanguages.length > 0 || microphoneLanguages.length > 0) && (
          <div className="cloud-transport-hint" role="note">
            <small>
              {t("settings.recognition.autoOnlyPreservedHint", {
                languages: preserved,
              })}
            </small>
          </div>
        )}
      </>
    );
  }

  return (
    <>
      <RecognitionLanguageSelect
        label={t("settings.recognition.speakerLanguage")}
        value={speakerLanguages}
        mode={mode}
        model={model}
        disabled={disabled}
        floating={floating}
        supportedLanguages={supportedLanguages}
        onChange={onChangeSpeaker}
      />
      <RecognitionLanguageSelect
        label={t("settings.recognition.microphoneLanguage")}
        value={microphoneLanguages}
        mode={mode}
        model={model}
        disabled={disabled}
        floating={floating}
        supportedLanguages={supportedLanguages}
        onChange={onChangeMicrophone}
      />
    </>
  );
}
