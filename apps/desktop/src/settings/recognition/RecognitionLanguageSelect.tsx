import { useTranslation } from "react-i18next";

import type { Settings } from "../types";
import { Select } from "../SettingsControls";

export function RecognitionLanguageSelect({
  value,
  disabled,
  supportedLanguages,
  onChange,
}: {
  value: Settings["asr"]["language"];
  disabled: boolean;
  supportedLanguages?: readonly string[];
  onChange: (value: Settings["asr"]["language"]) => void;
}) {
  const { t } = useTranslation();
  const options = [
    { value: "auto", label: t("languages.auto") },
    { value: "en", label: t("languages.english") },
    { value: "ja", label: t("languages.japanese") },
    { value: "zh", label: t("languages.chinese") },
    { value: "ko", label: t("languages.korean") },
    { value: "es", label: t("languages.spanish") },
    { value: "fr", label: t("languages.french") },
    { value: "de", label: t("languages.german") },
  ];

  return (
    <Select
      label={t("settings.recognition.language")}
      value={value}
      options={supportedLanguages?.length
        ? options.filter((option) => supportedLanguages.includes(option.value)) : options}
      placeholder={options.find((option) => option.value === value)?.label ?? value}
      disabled={disabled}
      onChange={(language) => onChange(language as Settings["asr"]["language"])}
    />
  );
}
