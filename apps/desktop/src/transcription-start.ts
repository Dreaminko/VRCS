export type TranscriptionStartBehavior = "continue_current" | "new_conversation";

export const DEFAULT_TRANSCRIPTION_START_BEHAVIOR: TranscriptionStartBehavior =
  "continue_current";

const STORAGE_KEY = "vrcs.transcriptionStartBehavior";

export function normalizeTranscriptionStartBehavior(
  value: unknown,
): TranscriptionStartBehavior {
  return value === "new_conversation"
    ? "new_conversation"
    : DEFAULT_TRANSCRIPTION_START_BEHAVIOR;
}

export function readTranscriptionStartBehavior(): TranscriptionStartBehavior {
  try {
    return normalizeTranscriptionStartBehavior(
      globalThis.localStorage?.getItem(STORAGE_KEY),
    );
  } catch {
    return DEFAULT_TRANSCRIPTION_START_BEHAVIOR;
  }
}

export function writeTranscriptionStartBehavior(
  behavior: TranscriptionStartBehavior,
): void {
  try {
    globalThis.localStorage?.setItem(
      STORAGE_KEY,
      normalizeTranscriptionStartBehavior(behavior),
    );
  } catch {
    // Keep the setting usable for this session when storage is unavailable.
  }
}

export function shouldCreateConversationOnCaptureToggle(
  running: boolean,
  behavior: unknown,
): boolean {
  return !running
    && normalizeTranscriptionStartBehavior(behavior) === "new_conversation";
}
