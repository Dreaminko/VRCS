import { useCallback, useMemo, useRef, useState } from "react";
import type { ActiveLanguageSession } from "../core-client/types";
import type { TranslationSettings } from "../settings/types";
import { defaultHiddenSourceLanguages, parsePlayerHistory, PLAYER_HISTORY_KEY, type PlayerHistoryPreferences } from "./player-history";

/** Owned by the workspace so switching between chat and captions keeps one policy. */
export function usePlayerHistoryPreferences(translation?: TranslationSettings, session?: ActiveLanguageSession) {
  const [preferences, setPreferences] = useState(() => {
    try { return parsePlayerHistory(localStorage.getItem(PLAYER_HISTORY_KEY)); }
    catch { return parsePlayerHistory(null); }
  });
  const current = useRef(preferences);
  const getCurrent = useCallback(() => current.current, []);
  const update = useCallback((change: PlayerHistoryPreferences | ((current: PlayerHistoryPreferences) => PlayerHistoryPreferences)) => {
    const value = typeof change === "function" ? change(current.current) : change;
    current.current = value;
    setPreferences(value);
    try { localStorage.setItem(PLAYER_HISTORY_KEY, JSON.stringify(value)); }
    catch { /* Keep the current view usable without browser storage. */ }
  }, []);
  const defaultLanguages = useMemo(() => defaultHiddenSourceLanguages(translation, session), [translation, session]);
  return {
    preferences, update, getCurrent, defaultLanguages,
    hiddenTracks: preferences.hiddenTracks,
    hiddenSourceLanguages: preferences.hiddenSourceLanguages ?? defaultLanguages,
  };
}
