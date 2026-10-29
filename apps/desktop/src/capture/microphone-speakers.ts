export type SpeakerDecision = "allow" | "block";

export interface MicrophoneSpeaker {
  id: string;
  index: number;
  decision: SpeakerDecision;
  last_active_at: string;
}

export interface MicrophoneSpeakerSnapshot {
  enabled: boolean;
  supported: boolean;
  running: boolean;
  generation: string;
  revision: number;
  snapshot_sequence: number;
  speakers: MicrophoneSpeaker[];
}

export type MicrophoneSpeakerAction =
  | { set_microphone_filter: boolean }
  | { set_microphone_speaker: { generation: string; revision: number; id: string; decision: SpeakerDecision } };

export function isMicrophoneSpeakerSnapshot(value: unknown): value is MicrophoneSpeakerSnapshot {
  if (typeof value !== "object" || value === null) return false;
  const s = value as MicrophoneSpeakerSnapshot;
  return typeof s.enabled === "boolean" && typeof s.supported === "boolean" && typeof s.running === "boolean"
    && typeof s.generation === "string" && Number.isSafeInteger(s.revision) && s.revision >= 0
    && Number.isSafeInteger(s.snapshot_sequence) && s.snapshot_sequence >= 0 && Array.isArray(s.speakers) && s.speakers.length <= 32
    && s.speakers.every(item => item && typeof item.id === "string" && Number.isSafeInteger(item.index) && item.index >= 0
      && ["allow", "block"].includes(item.decision) && typeof item.last_active_at === "string");
}
