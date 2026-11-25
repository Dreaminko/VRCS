import { useMemo } from "react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import {
  canonicalLanguageTag,
  languageSearchText,
  localizedLanguageName,
  translationLanguage,
} from "../../translation/translation-languages";
import { DropdownField } from "./DropdownField";
import type { DropdownOption } from "./DropdownField";

export function LanguagePicker({
  label,
  helper,
  value,
  placeholder,
  languageCodes,
  leadingOptions,
  allowCustom = false,
  disabled = false,
  compact = false,
  floating = false,
  icon,
  iconOnly = false,
  onChange,
}: {
  label: string;
  helper?: string;
  value: string;
  placeholder?: string;
  languageCodes: readonly string[];
  leadingOptions?: readonly DropdownOption[];
  allowCustom?: boolean;
  disabled?: boolean;
  compact?: boolean;
  floating?: boolean;
  icon?: ReactNode;
  iconOnly?: boolean;
  onChange: (value: string) => void;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const options = useMemo(() => {
    const leading = leadingOptions ?? [];
    const leadingValues = new Set(leading.map((option) => option.value));
    const codes = Array.from(new Set([
      ...(leadingValues.has(value) ? [] : [value]),
      ...languageCodes,
    ].filter(Boolean)));
    return [...leading, ...codes.map((code) => languageOption(code, locale))];
  }, [languageCodes, leadingOptions, locale, value]);

  const picker = (
    <DropdownField
      label={label}
      value={value}
      placeholder={placeholder}
      options={options}
      disabled={disabled}
      compact={compact}
      floating={floating}
      floatingWidth={compact ? 300 : undefined}
      icon={icon}
      iconOnly={iconOnly}
      searchable
      searchPlaceholder={t("translation.languagePicker.search")}
      emptyLabel={t("translation.languagePicker.empty")}
      createOption={allowCustom ? (query) => {
        const code = canonicalLanguageTag(query);
        if (!code || languageCodes.includes(code)) return null;
        return {
          value: code,
          label: localizedLanguageName(code, locale),
          description: t("translation.languagePicker.useCustom", { code }),
          searchText: query,
        };
      } : undefined}
      onChange={onChange}
    />
  );

  if (compact) return picker;
  return (
    <div className="field language-picker-field">
      <span>{label}</span>
      {picker}
      {helper && <small>{helper}</small>}
    </div>
  );
}

function languageOption(code: string, locale: string): DropdownOption {
  const language = translationLanguage(code);
  const localizedName = localizedLanguageName(code, locale);
  return {
    value: code,
    label: localizedName,
    description: language && language.nativeName !== localizedName
      ? `${language.nativeName} · ${code}`
      : code,
    searchText: language ? languageSearchText(language, locale) : code,
  };
}
