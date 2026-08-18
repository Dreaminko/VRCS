import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { localizedError } from "../../app/app-utils";
import {
  defaultDesktopPreferences,
  loadDesktopPreferences,
  updateDesktopPreference,
} from "../../desktop-preferences";
import type { DesktopPreferences } from "../../desktop-preferences";
import {
  changeUiLanguage,
  currentUiLanguagePreference,
} from "../../i18n";
import type { UiLanguagePreference } from "../../app/ui-language";
import type { SaveState } from "../settings-types";

export function useDesktopPreferences() {
  const { t } = useTranslation();
  const [desktopPreferences, setDesktopPreferences] = useState(defaultDesktopPreferences);
  const [ready, setReady] = useState(false);
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const [message, setMessage] = useState("");
  const [uiLanguagePreference, setUiLanguagePreference] = useState<UiLanguagePreference>(
    currentUiLanguagePreference,
  );

  useEffect(() => {
    let cancelled = false;
    void loadDesktopPreferences().then(
      (saved) => {
        if (cancelled) return;
        setDesktopPreferences(saved);
        setReady(true);
      },
      (reason) => {
        if (cancelled) return;
        setMessage(localizedError(reason, t, "errors.desktop.read"));
        setSaveState("error");
        setReady(true);
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);

  const updateDesktop = async (key: keyof DesktopPreferences, enabled: boolean) => {
    const previous = desktopPreferences;
    const optimistic = { ...previous, [key]: enabled };
    setDesktopPreferences(optimistic);
    setSaveState("saving");
    setMessage("");
    try {
      const saved = await updateDesktopPreference(previous, key, enabled);
      setDesktopPreferences(saved);
      setSaveState("saved");
    } catch (reason) {
      setDesktopPreferences(previous);
      setMessage(localizedError(reason, t, "errors.desktop.save"));
      setSaveState("error");
    }
  };

  const updateUiLanguage = async (preference: UiLanguagePreference) => {
    const previous = uiLanguagePreference;
    setUiLanguagePreference(preference);
    setSaveState("saving");
    setMessage("");
    try {
      await changeUiLanguage(preference);
      setSaveState("saved");
    } catch (reason) {
      setUiLanguagePreference(previous);
      setMessage(localizedError(reason, t, "errors.desktop.language"));
      setSaveState("error");
    }
  };

  return {
    desktopPreferences,
    ready,
    saveState,
    message,
    uiLanguagePreference,
    updateDesktop,
    updateUiLanguage,
  };
}
