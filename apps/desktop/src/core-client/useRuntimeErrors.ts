import { useCallback, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { localizedError } from "../app/app-utils";

export type ReportRuntimeError = (
  reason: unknown,
  fallbackKey: string,
  source?: string,
) => void;

interface RuntimeErrorEntry {
  message: string;
  code: string | null;
}

function extractErrorCode(reason: unknown): string | null {
  if (
    reason
    && typeof reason === "object"
    && "code" in reason
    && typeof reason.code === "string"
    && reason.code
  ) {
    return reason.code;
  }
  return null;
}

export function useRuntimeErrors() {
  const { t } = useTranslation();
  const tRef = useRef(t);
  tRef.current = t;
  const [errors, setErrors] = useState<Map<string, RuntimeErrorEntry>>(() => new Map());
  const latest = [...errors.values()].at(-1) ?? null;
  const error = latest?.message ?? null;
  const errorCode = latest?.code ?? null;

  const reportError = useCallback<ReportRuntimeError>((
    reason,
    fallbackKey,
    source = "general",
  ) => {
    const message = localizedError(reason, tRef.current, fallbackKey);
    const code = extractErrorCode(reason);
    setErrors((current) => {
      const next = new Map(current);
      next.delete(source);
      next.set(source, { message, code });
      return next;
    });
  }, []);

  const reportMessage = useCallback((source: string, message: string) => {
    setErrors((current) => {
      const next = new Map(current);
      next.delete(source);
      next.set(source, { message, code: null });
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
    errorCode,
    reportError,
    reportMessage,
    clearError,
    clearErrorFrom,
  };
}
