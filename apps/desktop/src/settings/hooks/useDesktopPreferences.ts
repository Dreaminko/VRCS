import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { localizedError } from "../../app/app-utils";
import {
  defaultDesktopPreferences,
  loadDesktopPreferences,
  updateDesktopPreference,
} from "../../shell/desktop-preferences";
import type { DesktopPreferences } from "../../shell/desktop-preferences";
import {
  changeUiLanguage,
  currentUiLanguagePreference,
} from "../../i18n";
import type { UiLanguagePreference } from "../../app/ui-language";

export function useDesktopPreferences() {
  const { t } = useTranslation();
  const [desktopPreferences, setDesktopPreferences] = useState(defaultDesktopPreferences);
  const [ready, setReady] = useState(false);
  const [uiLanguageSaving, setUiLanguageSaving] = useState(false);
  const [message, setMessage] = useState("");
  const confirmedPreferences = useRef(defaultDesktopPreferences);
  const saveQueue = useRef(Promise.resolve());
  const versions = useRef({ launchAtStartup: 0, runAsAdministrator: 0, minimizeToTray: 0 });
  const [uiLanguagePreference, setUiLanguagePreference] = useState<UiLanguagePreference>(
    currentUiLanguagePreference,
  );

  useEffect(() => {
    let cancelled = false;
    void loadDesktopPreferences().then(
      (saved) => {
        if (cancelled) return;
        confirmedPreferences.current = saved;
        setDesktopPreferences(saved);
        setReady(true);
      },
      (reason) => {
        if (cancelled) return;
        setMessage(localizedError(reason, t, "errors.desktop.read"));
        setReady(true);
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);

  const updateDesktop = async (key: keyof DesktopPreferences, enabled: boolean) => {
    const version = ++versions.current[key];
    setDesktopPreferences((current) => ({ ...current, [key]: enabled }));
    setMessage("");
    // Serialize native writes; a failed save must not roll back a newer choice.
    const save = saveQueue.current.then(async () => {
      try {
        confirmedPreferences.current = await updateDesktopPreference(confirmedPreferences.current, key, enabled);
      } catch (reason) {
        if (version !== versions.current[key]) return;
        const previous = confirmedPreferences.current[key];
        setDesktopPreferences((current) => ({ ...current, [key]: previous }));
        setMessage(localizedError(reason, t, "errors.desktop.save"));
      }
    });
    saveQueue.current = save;
    await save;
  };

  const updateUiLanguage = async (preference: UiLanguagePreference) => {
    const previous = uiLanguagePreference;
    setUiLanguagePreference(preference);
    setUiLanguageSaving(true);
    setMessage("");
    try {
      await changeUiLanguage(preference);
    } catch (reason) {
      setUiLanguagePreference(previous);
      setMessage(localizedError(reason, t, "errors.desktop.language"));
    } finally {
      setUiLanguageSaving(false);
    }
  };

  return {
    desktopPreferences,
    ready,
    uiLanguageSaving,
    message,
    uiLanguagePreference,
    updateDesktop,
    updateUiLanguage,
  };
}
