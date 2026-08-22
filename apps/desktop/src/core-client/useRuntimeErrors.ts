import { useCallback, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { localizedError } from "../app/app-utils";

export type ReportRuntimeError = (
  reason: unknown,
  fallbackKey: string,
  source?: string,
) => void;

export function useRuntimeErrors() {
  const { t } = useTranslation();
  const tRef = useRef(t);
  tRef.current = t;
  const [errors, setErrors] = useState<Map<string, string>>(() => new Map());
  const error = [...errors.values()].at(-1) ?? null;

  const reportError = useCallback<ReportRuntimeError>((
    reason,
    fallbackKey,
    source = "general",
  ) => {
    const message = localizedError(reason, tRef.current, fallbackKey);
    setErrors((current) => {
      const next = new Map(current);
      next.delete(source);
      next.set(source, message);
      return next;
    });
  }, []);

  const reportMessage = useCallback((source: string, message: string) => {
    setErrors((current) => {
      const next = new Map(current);
      next.delete(source);
      next.set(source, message);
      return next;
    });
  }, []);

  const clearError = useCallback(() => setErrors(new Map()), []);

  const clearErrorFrom = useCallback((source: string) => {
    setErrors((current) => {
      if (!current.has(source)) return current;
      const next = new Map(current);
      next.delete(source);
      return next;
    });
  }, []);

  return {
    error,
    reportError,
    reportMessage,
    clearError,
    clearErrorFrom,
  };
}
