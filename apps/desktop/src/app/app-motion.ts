const REDUCED_MOTION_STORAGE_KEY = "vrcs.reducedMotion";
const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

export function readReducedMotionPreference(): boolean {
  try {
    return window.localStorage.getItem(REDUCED_MOTION_STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

let preference = readReducedMotionPreference();
const listeners = new Set<() => void>();

export function currentReducedMotionPreference(): boolean {
  return preference;
}

export function prefersReducedMotion(): boolean {
  return preference || (typeof matchMedia === "function" && matchMedia(REDUCED_MOTION_QUERY).matches);
}

function applyMotionPreference(): void {
  if (typeof document === "undefined") return;
  const reduced = prefersReducedMotion();
  document.documentElement.dataset.reducedMotion = String(reduced);
  if (!reduced) return;

  // Settle active motion as well as skipping the next animation.
  for (const animation of document.getAnimations()) {
    if (animation.effect?.getComputedTiming().endTime !== Infinity) animation.finish();
  }
}

function notifyListeners(): void {
  for (const listener of listeners) listener();
}

export function updateReducedMotionPreference(enabled: boolean): void {
  preference = enabled;
  try {
    window.localStorage.setItem(REDUCED_MOTION_STORAGE_KEY, String(enabled));
  } catch {
    // Keep the current preference when storage is unavailable.
  }
  applyMotionPreference();
  notifyListeners();
}

export function subscribeReducedMotionPreference(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function initializeMotionPreference(): () => void {
  preference = readReducedMotionPreference();
  applyMotionPreference();
  const media = window.matchMedia(REDUCED_MOTION_QUERY);
  const onStorage = (event: StorageEvent) => {
    if (event.key !== null && event.key !== REDUCED_MOTION_STORAGE_KEY) return;
    preference = readReducedMotionPreference();
    applyMotionPreference();
    notifyListeners();
  };
  window.addEventListener("storage", onStorage);
  media.addEventListener("change", applyMotionPreference);
  return () => {
    window.removeEventListener("storage", onStorage);
    media.removeEventListener("change", applyMotionPreference);
  };
}
