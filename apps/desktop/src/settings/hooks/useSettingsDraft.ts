import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { localizedError } from "../../app/app-utils";
import type { SettingsUpdateOptions } from "../settings-autosave";
import type { Settings } from "../types";
import type { ApplySettings, SaveState } from "../settings-types";

export type SettingsSaveHandler = ((value: Settings) => Promise<Settings>) & {
  update?: (
    update: (current: Settings) => Settings,
    options?: SettingsUpdateOptions<Settings>,
  ) => Promise<Settings>;
};

export function useSettingsDraft(
  settings: Settings,
  onSave: SettingsSaveHandler,
) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState(settings);
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const [saveMessage, setSaveMessage] = useState("");
  const draftRef = useRef(settings);
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const saveVersionRef = useRef(0);
  const pendingUpdatesRef = useRef<Array<{
    version: number;
    apply: (current: Settings) => Settings;
  }>>([]);

  const rebaseDraft = useCallback((base: Settings) => {
    const next = pendingUpdatesRef.current.reduce(
      (current, entry) => entry.apply(current),
      base,
    );
    draftRef.current = next;
    setDraft(next);
    return next;
  }, []);

  useEffect(() => {
    rebaseDraft(settings);
  }, [rebaseDraft, settings]);

  const applySettings: ApplySettings = useCallback((update, afterSave, afterError) => {
    const base = draftRef.current;
    const next = update(base);
    const version = ++saveVersionRef.current;
    pendingUpdatesRef.current.push({ version, apply: update });
    draftRef.current = next;
    setDraft(next);
    setSaveState("saving");
    setSaveMessage("");
    const request = onSave.update
      ? onSave.update(update, { coalesce: true, base: settingsRef.current })
      : onSave(next);
    void request.then(
      (saved) => {
        pendingUpdatesRef.current = pendingUpdatesRef.current.filter(
          (entry) => entry.version > version,
        );
        if (version === saveVersionRef.current) {
          draftRef.current = saved;
          setDraft(saved);
          setSaveState(saved === settingsRef.current && next !== saved ? "idle" : "saved");
        } else {
          rebaseDraft(saved);
        }
        afterSave?.();
      },
      (reason) => {
        pendingUpdatesRef.current = pendingUpdatesRef.current.filter(
          (entry) => entry.version > version,
        );
        rebaseDraft(settingsRef.current);
        if (version === saveVersionRef.current) {
          setSaveMessage(localizedError(reason, t, "errors.settings.apply"));
          setSaveState("error");
        }
        afterError?.();
      },
    );
  }, [onSave, rebaseDraft, t]);

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
