import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { localizedError } from "../../app-utils";
import type { Settings } from "../../types";
import type { ApplySettings, SaveState } from "../settings-types";

export function useSettingsDraft(
  settings: Settings,
  onSave: (value: Settings) => Promise<Settings>,
) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState(settings);
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const [saveMessage, setSaveMessage] = useState("");
  const draftRef = useRef(settings);
  const saveVersionRef = useRef(0);
  const savingRef = useRef(false);

  useEffect(() => {
    draftRef.current = settings;
    if (savingRef.current) return;
    setDraft(settings);
  }, [settings]);

  const applySettings: ApplySettings = useCallback((update, afterSave) => {
    const next = update(draftRef.current);
    const version = ++saveVersionRef.current;
    savingRef.current = true;
    draftRef.current = next;
    setDraft(next);
    setSaveState("saving");
    setSaveMessage("");
    void onSave(next).then(
      (saved) => {
        if (version !== saveVersionRef.current) return;
        savingRef.current = false;
        draftRef.current = saved;
        setDraft(saved);
        setSaveState("saved");
        afterSave?.();
      },
      (reason) => {
        if (version !== saveVersionRef.current) return;
        savingRef.current = false;
        setSaveMessage(localizedError(reason, t, "errors.settings.apply"));
        setSaveState("error");
      },
    );
  }, [onSave, t]);

  const setFailure = useCallback((message: string) => {
    setSaveMessage(message);
    setSaveState("error");
  }, []);

  const getCurrent = useCallback(() => draftRef.current, []);

  return {
    draft,
    saveState,
    saveMessage,
    applySettings,
    getCurrent,
    setFailure,
  };
}

export type SettingsDraftController = ReturnType<typeof useSettingsDraft>;
