type NetworkCaptureSettings = {
  audio: { output: { mode: string; player_audio?: { enabled?: boolean } } };
  vrchat_chatbox?: { enabled: boolean };
};

export function usesVrchatNetwork(settings: NetworkCaptureSettings | null | undefined): boolean {
  return settings?.audio.output.player_audio?.enabled === true
    && (settings.audio.output.mode === "vrchat_players" || settings.vrchat_chatbox?.enabled === true);
}
