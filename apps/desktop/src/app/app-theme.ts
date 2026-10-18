import { prefersReducedMotion } from "./app-motion.ts";

export type ThemePreference = "system" | "light" | "dark";
export type AppTheme = "light" | "dark";

const THEME_STORAGE_KEY = "vrcs.theme";
const THEME_COLOR_DARK = "#282c33";
const THEME_COLOR_LIGHT = "#f5f8fb";

export function isThemePreference(value: unknown): value is ThemePreference {
  return value === "system" || value === "light" || value === "dark";
}

export function readThemePreference(): ThemePreference {
  try {
    const stored = window.localStorage.getItem(THEME_STORAGE_KEY);
    return isThemePreference(stored) ? stored : "system";
  } catch {
    return "system";
  }
}

export function writeThemePreference(preference: ThemePreference): void {
  try {
    window.localStorage.setItem(THEME_STORAGE_KEY, preference);
  } catch {
    // Ignore unavailable storage; the in-memory preference remains active.
  }
}

export function resolveTheme(preference: ThemePreference, systemPrefersDark: boolean): AppTheme {
  if (preference === "system") return systemPrefersDark ? "dark" : "light";
  return preference;
}

function systemPrefersDark(): boolean {
  return typeof matchMedia === "function" && matchMedia("(prefers-color-scheme: dark)").matches;
}

function setThemeAttribute(theme: AppTheme): void {
  if (typeof document === "undefined") return;
  document.documentElement.setAttribute("data-theme", theme);
}

/** Runs update inside a whole-page cross-fade so colors change coherently. */
function crossFade(update: () => void): void {
  if (typeof document === "undefined") {
    update();
    return;
  }
  const startViewTransition = (document as Document & {
    startViewTransition?: (update: () => void) => unknown;
  }).startViewTransition;
  if (!prefersReducedMotion() && typeof startViewTransition === "function") {
    startViewTransition.call(document, update);
  } else {
    update();
  }
}

export function applyTheme(theme: AppTheme): void {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  document
    .querySelector('meta[name="theme-color"]')
    ?.setAttribute("content", theme === "dark" ? THEME_COLOR_DARK : THEME_COLOR_LIGHT);
  if (root.getAttribute("data-theme") === theme) return;
  crossFade(() => setThemeAttribute(theme));
}

type ThemePreferenceListener = (preference: ThemePreference) => void;
const listeners = new Set<ThemePreferenceListener>();

/**
 * Flips the document theme and notifies subscribers inside the same
 * cross-fade update, so the toggle button and the background repaint in the
 * same frame instead of the button changing before the captured old state.
 */
function commitPreferenceChange(preference: ThemePreference): void {
  crossFade(() => {
    setThemeAttribute(resolveTheme(preference, systemPrefersDark()));
    for (const listener of listeners) listener(preference);
  });
}

function handleStorageChange(event: StorageEvent) {
  if (event.key !== THEME_STORAGE_KEY) return;
  commitPreferenceChange(readThemePreference());
}

/** Persists the preference and notifies subscribers in this window. */
export function updateThemePreference(preference: ThemePreference): void {
  writeThemePreference(preference);
  commitPreferenceChange(preference);
}

/** Subscribes to preference changes, including writes from other windows. */
export function subscribeThemePreference(listener: ThemePreferenceListener): () => void {
  listeners.add(listener);
  if (listeners.size === 1 && typeof window !== "undefined") {
    window.addEventListener("storage", handleStorageChange);
  }
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && typeof window !== "undefined") {
      window.removeEventListener("storage", handleStorageChange);
    }
  };
}
