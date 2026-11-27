type NetworkCaptureSettings = {
  audio?: { output?: { mode?: string; player_audio?: { enabled?: boolean } } };
  vrchat_chatbox?: { enabled?: boolean };
};

export function networkCaptureFeatureEnabled(settings: NetworkCaptureSettings | null | undefined): boolean {
  return settings?.audio?.output?.player_audio?.enabled === true;
}

export function playerAudioTrackEnabled(settings: NetworkCaptureSettings | null | undefined): boolean {
  return networkCaptureFeatureEnabled(settings) && settings?.audio?.output?.mode === "vrchat_players";
}

export function usesVrchatNetwork(settings: NetworkCaptureSettings | null | undefined): boolean {
  return networkCaptureFeatureEnabled(settings)
    && (settings?.audio?.output?.mode === "vrchat_players" || settings?.vrchat_chatbox?.enabled === true);
}
