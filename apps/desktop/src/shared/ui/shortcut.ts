export interface ShortcutKeyEvent {
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
  repeat?: boolean;
  isComposing?: boolean;
  getModifierState?: (key: string) => boolean;
}

export function shortcutModifiers(event: ShortcutKeyEvent): string[] {
  return [event.ctrlKey && "Ctrl", event.altKey && "Alt", event.shiftKey && "Shift"]
    .filter((modifier): modifier is string => Boolean(modifier));
}

export function isShortcutModifier(key: string): boolean {
  return ["Control", "Alt", "Shift", "Meta", "AltGraph"].includes(key);
}

// Keep this format aligned with desktop_capture::shortcut::Shortcut::parse.
export function captureShortcut(event: ShortcutKeyEvent): string | null {
  if (event.repeat || event.isComposing || event.metaKey || event.getModifierState?.("AltGraph")
    || ["Dead", "Process", "Unidentified"].includes(event.key)) return null;
  const modifiers = shortcutModifiers(event);
  if (!modifiers.length) return null;
  let key: string;
  if (/^[a-z0-9]$/i.test(event.key)) key = event.key.toUpperCase();
  else if (/^Digit[0-9]$/.test(event.code)) key = event.code.slice(5);
  else if (/^Key[A-Z]$/.test(event.code)) key = event.code.slice(3);
  else if (/^F([1-9]|1[01])$/.test(event.key)) key = event.key;
  else return null;
  return [...modifiers, key].join("+");
}
