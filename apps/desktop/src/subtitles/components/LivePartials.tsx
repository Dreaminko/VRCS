import { MessageSquare } from "lucide-react";
import { useTranslation } from "react-i18next";

import { contentLanguageTag } from "../../app/ui-language";
import { livePartialHasSubtitle, useLivePartials } from "../../core-client/realtime-state";
import type { SpeakerIdentity, Subtitle } from "../types";
import { useSpeakerIdentity } from "../speaker-state";
import type { PlayerAudioStatus } from "../../core-client/types";
import { isAudioTrack, livePartialVisible } from "../player-history";

function isWaiting(phase?: PlayerAudioStatus["phase"]) {
  return phase?.startsWith("waiting_") || phase === "acquiring_key" || phase === "retrying_key";
}

function SpeakerLabel({ identity }: { identity: SpeakerIdentity }) {
  const { t } = useTranslation();
  const speaker = useSpeakerIdentity(identity)!;
  return <div className="message-meta"><span className="speaker-name">{speaker.display_name || t(isAudioTrack(speaker) ? "live.trackNumber" : "live.speakerNumber", { number: speaker.index + 1 })}</span></div>;
}

export function LivePartials({ subtitles, playerAudioPhase, hiddenTracks = [], hiddenSourceLanguages }: { subtitles: Subtitle[]; playerAudioPhase?: PlayerAudioStatus["phase"]; hiddenTracks?: readonly string[]; hiddenSourceLanguages?: readonly string[] }) {
  const { t } = useTranslation();
  const speaker = useLivePartials("speaker");
  const microphone = useLivePartials("microphone");
  const partials = [...speaker, ...microphone];
  if (!partials.length) {
    if (isWaiting(playerAudioPhase) && !subtitles.length) return null;
    return (
      <div className="message-group source-speaker streaming-message">
        <div className="bubble" role="status">{isWaiting(playerAudioPhase) ? t(`settings.audio.players.phase.${playerAudioPhase}`) : t("live.transcribing")}<span className="streaming-ellipsis" aria-hidden="true">…</span></div>
      </div>
    );
  }
  return partials.filter((partial) => livePartialVisible(partial, hiddenTracks, hiddenSourceLanguages)
    && !livePartialHasSubtitle(partial, subtitles)).map((partial) => {
    const preview = partial.conversation_preview ?? partial;
    const translation = preview.translation;
    if (!preview.text && !translation) return null;
    return (
      <div className={`message-group source-${partial.source} streaming-message`} key={`${partial.source}-${partial.utterance_id}`}>
        {partial.speaker && <SpeakerLabel identity={partial.speaker} />}
        <div className="bubble">
          {preview.text && <p className="bubble-original" lang={contentLanguageTag(partial.language)}>{preview.text}<span className="streaming-ellipsis" aria-hidden="true">…</span></p>}
          {preview.text && translation && <div className="bubble-translation-divider" aria-hidden="true" />}
          {translation && <p className="bubble-translation streaming-translation" lang={contentLanguageTag(partial.target_language)}>{translation}<span className="streaming-ellipsis" aria-hidden="true">…</span></p>}
        </div>
      </div>
    );
  });
}

export function EmptyLiveView({ running, playerAudioPhase }: { running: boolean; playerAudioPhase?: PlayerAudioStatus["phase"] }) {
  const { t } = useTranslation();
  return (
    <div className="empty-state">
      <MessageSquare size={22} />
      <p>{running ? isWaiting(playerAudioPhase) ? t(`settings.audio.players.phase.${playerAudioPhase}`) : t("live.listening") : t("live.startHint")}</p>
    </div>
  );
}
