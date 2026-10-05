import { MessageSquare } from "lucide-react";
import { useTranslation } from "react-i18next";

import { contentLanguageTag } from "../../app/ui-language";
import { livePartialHasSubtitle, useLivePartial } from "../../realtime-state";
import type { Subtitle } from "../types";

export function LivePartials({ subtitles }: { subtitles: Subtitle[] }) {
  const { t } = useTranslation();
  const speaker = useLivePartial("speaker");
  const microphone = useLivePartial("microphone");
  const partials = [speaker, microphone].flatMap((partial) => partial ? [partial] : []);
  if (!partials.length) {
    return (
      <div className="message-group source-speaker streaming-message">
        <div className="bubble">{t("live.transcribing")}<span className="streaming-ellipsis" aria-hidden="true">…</span></div>
      </div>
    );
  }
  return partials.filter((partial) => !livePartialHasSubtitle(partial, subtitles)).map((partial) => {
    const preview = partial.conversation_preview ?? partial;
    return (
      <div className={`message-group source-${partial.source} streaming-message`} key={`${partial.source}-${partial.utterance_id}`}>
        {partial.speaker && <div className="message-meta">{t("live.speakerNumber", { number: partial.speaker.index + 1 })}</div>}
        <div className="bubble">
          {preview.text && <p className="bubble-original" lang={contentLanguageTag(partial.language)}>{preview.text}<span className="streaming-ellipsis" aria-hidden="true">…</span></p>}
          {preview.text && preview.translation && <div className="bubble-translation-divider" aria-hidden="true" />}
          {preview.translation && <p className="bubble-translation streaming-translation" lang={contentLanguageTag(partial.target_language)}>{preview.translation}<span className="streaming-ellipsis" aria-hidden="true">…</span></p>}
        </div>
      </div>
    );
  });
}

export function EmptyLiveView({ running }: { running: boolean }) {
  const { t } = useTranslation();
  return (
    <div className="empty-state">
      <MessageSquare size={22} />
      <p>{running ? t("live.listening") : t("live.startHint")}</p>
    </div>
  );
}
