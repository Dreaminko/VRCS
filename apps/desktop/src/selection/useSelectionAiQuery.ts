import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { learningApi } from "../learning/api";
import { localizedError } from "../app/app-utils";
import type { LearningPreferences } from "../learning/hooks/useLearningWorkspace";
import type { SelectionTarget } from "../app/app-types";
import type { SelectionQueryResponse } from "../learning/types";
import { selectionAiConfigured, selectionQueryInput } from "./selection-ai";

type QueryState =
  | { status: "idle"; response: null; error: "" }
  | { status: "loading"; response: null; error: "" }
  | { status: "success"; response: SelectionQueryResponse; error: "" }
  | { status: "error"; response: null; error: string };

const IDLE_STATE: QueryState = { status: "idle", response: null, error: "" };

export function useSelectionAiQuery(preferences: LearningPreferences) {
  const { t } = useTranslation();
  const [state, setState] = useState<QueryState>(IDLE_STATE);
  const requestRef = useRef(0);
  const controllerRef = useRef<AbortController | null>(null);

  const reset = useCallback(() => {
    requestRef.current += 1;
    controllerRef.current?.abort();
    controllerRef.current = null;
    setState(IDLE_STATE);
  }, []);

  const ask = useCallback(async (target: SelectionTarget, question: string) => {
    if (!selectionAiConfigured(preferences) || !question.trim()) return;
    const requestId = ++requestRef.current;
    controllerRef.current?.abort();
    const controller = new AbortController();
    controllerRef.current = controller;
    setState({ status: "loading", response: null, error: "" });
    try {
      const response = await learningApi.querySelection(
        selectionQueryInput(target, question, preferences),
        controller.signal,
      );
      if (requestId === requestRef.current) {
        setState({ status: "success", response, error: "" });
      }
    } catch (reason) {
      if (controller.signal.aborted || requestId !== requestRef.current) return;
      setState({
        status: "error",
        response: null,
        error: localizedError(reason, t, "errors.learning.query"),
      });
    } finally {
      if (requestId === requestRef.current) controllerRef.current = null;
    }
  }, [preferences, t]);

  useEffect(() => reset, [reset]);

  return { ...state, ask, reset };
}
