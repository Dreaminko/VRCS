import { isTauri } from "@tauri-apps/api/core";

export const CURRENT_ONBOARDING_VERSION = 1;

const STORE_KEY = "onboarding";
const WEB_STORAGE_KEY = "vrcs.onboarding";

export interface OnboardingState {
  version: number;
  status: "in_progress" | "completed";
  currentStep: number;
}

export interface OnboardingStorage {
  read: () => Promise<unknown>;
  write: (state: OnboardingState) => Promise<void>;
}

let writeQueue: Promise<void> = Promise.resolve();

const storage: OnboardingStorage = {
  read: async () => {
    if (!isTauri()) return globalThis.localStorage?.getItem(WEB_STORAGE_KEY);
    const { load } = await import("@tauri-apps/plugin-store");
    const store = await load("preferences.json", { autoSave: false });
    return store.get(STORE_KEY);
  },
  write: async (state) => {
    if (!isTauri()) {
      globalThis.localStorage?.setItem(WEB_STORAGE_KEY, JSON.stringify(state));
      return;
    }
    const { load } = await import("@tauri-apps/plugin-store");
    const store = await load("preferences.json", { autoSave: false });
    await store.set(STORE_KEY, state);
    await store.save();
  },
};

export function normalizeOnboardingState(value: unknown): OnboardingState {
  let parsed = value;
  if (typeof value === "string") {
    try {
      parsed = JSON.parse(value);
    } catch {
      parsed = null;
    }
  }
  if (!parsed || typeof parsed !== "object") {
    return { version: 0, status: "in_progress", currentStep: 0 };
  }
  const candidate = parsed as Partial<OnboardingState>;
  return {
    version: typeof candidate.version === "number" && Number.isInteger(candidate.version)
      ? Math.max(0, candidate.version)
      : 0,
    status: candidate.status === "completed" ? "completed" : "in_progress",
    currentStep: typeof candidate.currentStep === "number" && Number.isInteger(candidate.currentStep)
      ? Math.max(0, candidate.currentStep)
      : 0,
  };
}

export async function loadOnboardingState(adapter: OnboardingStorage = storage): Promise<OnboardingState> {
  return normalizeOnboardingState(await adapter.read());
}

export function needsOnboarding(state: OnboardingState): boolean {
  return state.version < CURRENT_ONBOARDING_VERSION || state.status !== "completed";
}

function queueWrite(state: OnboardingState, adapter: OnboardingStorage): Promise<void> {
  const request = writeQueue.then(() => adapter.write(state));
  writeQueue = request.catch(() => undefined);
  return request;
}

export function saveOnboardingProgress(
  currentStep: number,
  adapter: OnboardingStorage = storage,
): Promise<void> {
  return queueWrite({
    version: CURRENT_ONBOARDING_VERSION,
    status: "in_progress",
    currentStep: Math.max(0, Math.floor(currentStep)),
  }, adapter);
}

export function completeOnboarding(adapter: OnboardingStorage = storage): Promise<void> {
  return queueWrite({
    version: CURRENT_ONBOARDING_VERSION,
    status: "completed",
    currentStep: 0,
  }, adapter);
}
