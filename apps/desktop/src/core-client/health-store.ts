import { useCallback, useRef, useSyncExternalStore } from "react";
import type { Health } from "./types";

let currentHealth: Health | null = null;
let currentHealthJson: string | null = null;
const listeners = new Set<() => void>();

export function getCurrentHealth(): Health | null {
  return currentHealth;
}

export function publishHealth(next: Health | null): void {
  const serialized = next ? JSON.stringify(next) : null;
  if (serialized === currentHealthJson) return;
  currentHealthJson = serialized;
  currentHealth = next;
  for (const listener of listeners) listener();
}

export function subscribeHealth(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useHealth(): Health | null {
  return useSyncExternalStore(subscribeHealth, getCurrentHealth);
}

export function useHealthSelector<T>(
  selector: (health: Health | null) => T,
  isEqual: (previous: T, next: T) => boolean = Object.is,
): T {
  const selectorRef = useRef(selector);
  const isEqualRef = useRef(isEqual);
  selectorRef.current = selector;
  isEqualRef.current = isEqual;
  const cacheRef = useRef<{
    hasValue: boolean;
    health: Health | null;
    selector: (health: Health | null) => T;
    value: T;
  }>({
    hasValue: false,
    health: null,
    selector,
    value: undefined as T,
  });

  const getSnapshot = useCallback(() => {
    const health = getCurrentHealth();
    const currentSelector = selectorRef.current;
    const cached = cacheRef.current;
    if (cached.hasValue && cached.health === health && cached.selector === currentSelector) {
      return cached.value;
    }
    const next = currentSelector(health);
    if (cached.hasValue && isEqualRef.current(cached.value, next)) {
      cached.health = health;
      cached.selector = currentSelector;
      return cached.value;
    }
    cacheRef.current = {
      hasValue: true,
      health,
      selector: currentSelector,
      value: next,
    };
    return next;
  }, []);

  return useSyncExternalStore(subscribeHealth, getSnapshot);
}
