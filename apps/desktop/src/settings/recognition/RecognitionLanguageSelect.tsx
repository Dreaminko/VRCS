import { ArrowUp, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import {
  ASR_LANGUAGE_CODES,
  MAX_ASR_PREFERRED_LANGUAGES,
} from "../../providers/recognition-services";
import type { RecognitionLanguageMode } from "../../providers/types";
import { LanguagePicker } from "../../shared/ui/LanguagePicker";
import { localizedLanguageName } from "../../translation/translation-languages";
import { Select } from "../SettingsControls";

export function RecognitionLanguageSelect({
  value,
  mode = "single",
  model,
  disabled,
  floating,
  supportedLanguages,
  onChange,
}: {
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
  const labelFor = (code: string) => localizedLanguageName(code, locale);
  const allCodes = supportedLanguages?.length
    ? ASR_LANGUAGE_CODES.filter((code) => supportedLanguages.includes(code))
    : [...ASR_LANGUAGE_CODES];

  if (mode === "auto_only") {
    return (
      <div className="asr-language-field">
        <Select
          label={t("settings.recognition.language")}
          value="auto"
          options={[{ value: "auto", label: t("languages.auto") }]}
          disabled
          floating={floating}
          onChange={() => undefined}
        />
        {value.length > 0 && (
          <div className="cloud-transport-hint" role="note">
            <small>
              {t("settings.recognition.autoOnlyPreservedHint", {
                languages: value.map(labelFor).join(" / "),
              })}
            </small>
          </div>
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
  const removeAt = (index: number) => {
    onChange(value.filter((_, itemIndex) => itemIndex !== index));
  };

  return (
    <div className="asr-language-field">
      <div className="field">
        <div className="asr-language-header">
          <span>{t("settings.recognition.language")}</span>
          {value.length > 0 && (
            <button
              className="asr-language-clear"
              type="button"
              disabled={disabled}
              onClick={() => onChange([])}
            >
              {t("languages.auto")}
            </button>
          )}
        </div>
        {value.length > 0 && (
          <div className="asr-language-chips" role="list">
            {value.map((code, index) => (
              <span className="asr-language-chip" key={code} role="listitem">
                <span className="asr-language-rank">{index + 1}</span>
                <span>{labelFor(code)}</span>
                {index > 0 && (
                  <button
                    type="button"
                    disabled={disabled}
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
                  disabled={disabled}
                  aria-label={t("settings.recognition.removeLanguage", {
                    language: labelFor(code),
                  })}
                  onClick={() => removeAt(index)}
                >
                  <X size={12} aria-hidden="true" />
                </button>
              </span>
            ))}
          </div>
        )}
        <LanguagePicker
          label={t("settings.recognition.language")}
          value=""
          placeholder={
            value.length === 0
              ? t("languages.auto")
              : value.length >= MAX_ASR_PREFERRED_LANGUAGES
                ? t("settings.recognition.preferredLanguagesLimit", {
                    max: MAX_ASR_PREFERRED_LANGUAGES,
                  })
                : t("settings.recognition.addPreferredLanguage")
          }
          languageCodes={remainingCodes}
          leadingOptions={value.length > 0 ? [{ value: "auto", label: t("languages.auto") }] : undefined}
          compact
          disabled={disabled || value.length >= MAX_ASR_PREFERRED_LANGUAGES || remainingCodes.length === 0}
          floating={Boolean(floating)}
          onChange={(next) => {
            if (next === "auto") {
              onChange([]);
              return;
            }
            if (value.includes(next) || value.length >= MAX_ASR_PREFERRED_LANGUAGES) return;
            onChange([...value, next]);
          }}
        />
      </div>
      {mode === "single" && value.length > 1 && (
        <div className="cloud-transport-hint" role="note">
          <small>
            {t("settings.recognition.singleLanguageHint", {
              language: labelFor(value[0]),
            })}
          </small>
        </div>
      )}
      {mode === "preferred_list" && model?.startsWith("fun-asr-realtime") && value.length > 1 && (
        <div className="cloud-transport-hint" role="note">
          <small>
            {t("settings.recognition.funAsrLegacyHint", {
              language: labelFor(value[0]),
            })}
          </small>
        </div>
      )}
      {unsupportedPrimary && supportedLanguages && (
        <div className="cloud-transport-hint" role="note">
          <small>
            {t("settings.recognition.unsupportedPrimaryLanguageHint", {
              language: labelFor(value[0]),
              supported: supportedLanguages.map(labelFor).join(" / "),
            })}
          </small>
        </div>
      )}
    </div>
  );
}
