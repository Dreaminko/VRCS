import assert from "node:assert/strict";
import test from "node:test";

import {
  CURRENT_ONBOARDING_VERSION,
  completeOnboarding,
  loadOnboardingState,
  needsOnboarding,
  normalizeOnboardingState,
  saveOnboardingProgress,
  type OnboardingState,
  type OnboardingStorage,
} from "../src/onboarding/onboarding-state.ts";

function fakeStorage(initial: unknown) {
  let value = initial;
  const writes: OnboardingState[] = [];
  const storage: OnboardingStorage = {
    read: async () => value,
    write: async (state) => {
      value = state;
      writes.push(state);
    },
  };
  return { storage, writes };
}

test("requires onboarding when no valid state exists", async () => {
  const { storage } = fakeStorage(null);
  const state = await loadOnboardingState(storage);
  assert.equal(needsOnboarding(state), true);
  assert.deepEqual(state, { version: 0, status: "in_progress", currentStep: 0 });
});

test("normalizes saved web state and restores progress", () => {
  assert.deepEqual(normalizeOnboardingState(JSON.stringify({
    version: CURRENT_ONBOARDING_VERSION,
    status: "in_progress",
    currentStep: 3,
  })), {
    version: CURRENT_ONBOARDING_VERSION,
    status: "in_progress",
    currentStep: 3,
  });
});

test("saves progress and completion as versioned state", async () => {
  const { storage, writes } = fakeStorage(null);
  await saveOnboardingProgress(2, storage);
  await completeOnboarding(storage);
  assert.deepEqual(writes, [
    { version: CURRENT_ONBOARDING_VERSION, status: "in_progress", currentStep: 2 },
    { version: CURRENT_ONBOARDING_VERSION, status: "completed", currentStep: 0 },
  ]);
  assert.equal(needsOnboarding(writes[1]), false);
});
