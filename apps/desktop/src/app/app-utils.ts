import type { TFunction } from "i18next";

import { ApiError, formatApiErrorMessage } from "../core-client/api-error";

const timeFormatters = new Map<string, Intl.DateTimeFormat>();
const dateTimeFormatters = new Map<string, Intl.DateTimeFormat>();

function getDateTimeFormatter(
  cache: Map<string, Intl.DateTimeFormat>,
  locale: string,
  options: Intl.DateTimeFormatOptions,
): Intl.DateTimeFormat {
  let formatter = cache.get(locale);
  if (!formatter) {
    formatter = new Intl.DateTimeFormat(locale, options);
    if (cache.size >= 32) cache.clear();
    cache.set(locale, formatter);
  }
  return formatter;
}

export function timestamp(value: string, locale: string): string {
  return getDateTimeFormatter(timeFormatters, locale, {
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

export function localizedError(
  reason: unknown,
  t: TFunction,
  fallbackKey: string,
): string {
  const missing = "__VRCS_UNTRANSLATED_ERROR__";
  const lookupErrorCode = (code: string, params?: Record<string, unknown>) => {
    const direct = t(`errors.${code}`, { ...params, defaultValue: missing });
    if (direct !== missing) return direct;
    if (code.startsWith("asr.qwen_model.")) {
      return t(`errors.asr.model.${code.slice("asr.qwen_model.".length)}`, {
        ...params,
        defaultValue: missing,
      });
    }
    return missing;
  };
  if (reason instanceof ApiError) {
    if (reason.code.startsWith("platform.")) return t(reason.code);
    const localized = lookupErrorCode(reason.code, reason.params);
    if (localized === missing) return reason.detail || t(fallbackKey);
    return formatApiErrorMessage(
      reason,
      localized,
      (detail) => t("errors.audio.diagnostic_detail", { detail }),
    );
  }
  if (
    reason
    && typeof reason === "object"
    && "code" in reason
    && typeof reason.code === "string"
  ) {
    if (reason.code.startsWith("platform.")) return t(reason.code);
    const localized = lookupErrorCode(reason.code);
    if (localized !== missing) return localized;
    return "detail" in reason && typeof reason.detail === "string" && reason.detail
      ? reason.detail
      : t(fallbackKey);
  }
  if (typeof reason === "string") {
    if (reason.startsWith("platform.")) return t(reason);
    if (reason.startsWith("desktop_ocr.")) {
      const localized = t(`ocrWindow.errors.${reason.slice("desktop_ocr.".length)}`, { defaultValue: missing });
      if (localized !== missing) return localized;
    }
    if (reason.startsWith("desktop_vision.")) {
      const localized = t(`visionWindow.errors.${reason.slice("desktop_vision.".length)}`, { defaultValue: missing });
      if (localized !== missing) return localized;
    }
  }
  return reason instanceof Error ? reason.message : t(fallbackKey);
}

export function conversationTime(
  value: string,
  locale: string,
  todayLabel: string,
  yesterdayLabel: string,
) {
  const date = new Date(value);
  const today = new Date();
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  const sameDay = (left: Date, right: Date) => left.toDateString() === right.toDateString();
  if (sameDay(date, today)) return `${todayLabel} ${timestamp(value, locale)}`;
  if (sameDay(date, yesterday)) return `${yesterdayLabel} ${timestamp(value, locale)}`;
  return getDateTimeFormatter(dateTimeFormatters, locale, {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

export function contextExcerpt(context: string, term: string): string {
  return context.split(/(?<=[。！？.!?])/).find((sentence) => sentence.includes(term))?.trim() ?? context;
}
