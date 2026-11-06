import type { SettingsCategory } from "./settings-types";

const STORAGE_KEY = "vrcs.settingsTabOrder";

export const DEFAULT_SETTINGS_TAB_ORDER: SettingsCategory[] = [
  "system", "audio", "recognition", "translation", "vr_overlay", "vision",
  "glossary", "api", "learning", "connections", "about",
];

export function readSettingsTabOrder(): SettingsCategory[] {
  try {
    const stored: unknown = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? "null");
    const saved = Array.isArray(stored)
      ? stored.filter((id): id is SettingsCategory => DEFAULT_SETTINGS_TAB_ORDER.includes(id))
      : [];
    return [...new Set([...saved, ...DEFAULT_SETTINGS_TAB_ORDER])];
  } catch {
    return [...DEFAULT_SETTINGS_TAB_ORDER];
  }
}

export function saveSettingsTabOrder(order: SettingsCategory[]): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(order));
  } catch {
    // Keep the current order when local storage is unavailable.
  }
}

export function moveSettingsTab(
  order: SettingsCategory[], category: SettingsCategory, target: SettingsCategory,
): SettingsCategory[] {
  const from = order.indexOf(category);
  const to = order.indexOf(target);
  if (from < 0 || to < 0 || from === to) return order;
  const next = [...order];
  next.splice(from, 1);
  next.splice(to, 0, category);
  return next;
}
