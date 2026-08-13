import type { TFunction } from "i18next";

import { ApiError } from "./api-error";

export function timestamp(value: string, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
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
  if (reason instanceof ApiError) {
    const localized = t(`errors.${reason.code}`, {
      ...reason.params,
      defaultValue: missing,
    });
    return localized === missing ? reason.detail || t(fallbackKey) : localized;
  }
  if (
    reason
    && typeof reason === "object"
    && "code" in reason
    && typeof reason.code === "string"
  ) {
    const localized = t(`errors.${reason.code}`, { defaultValue: missing });
    if (localized !== missing) return localized;
    return "detail" in reason && typeof reason.detail === "string" && reason.detail
      ? reason.detail
      : t(fallbackKey);
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
  return new Intl.DateTimeFormat(locale, { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" }).format(date);
}

export function contextExcerpt(context: string, term: string): string {
  return context.split(/(?<=[。！？.!?])/).find((sentence) => sentence.includes(term))?.trim() ?? context;
}
