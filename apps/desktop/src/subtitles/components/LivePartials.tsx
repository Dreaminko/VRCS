import { MessageSquare } from "lucide-react";
import { useTranslation } from "react-i18next";

import { contentLanguageTag } from "../../app/ui-language";
import { useLivePartial } from "../../realtime-state";

export function LivePartials() {
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
  return partials.map((partial) => (
    <div className={`message-group source-${partial.source} streaming-message`} key={`${partial.source}-${partial.utterance_id}`}>
      {partial.speaker && <div className="message-meta">{t("live.speakerNumber", { number: partial.speaker.index + 1 })}</div>}
      <div className="bubble">
        {partial.text && <p className="bubble-original" lang={contentLanguageTag(partial.language)}>{partial.text}<span className="streaming-ellipsis" aria-hidden="true">…</span></p>}
        {partial.text && partial.translation && <div className="bubble-translation-divider" aria-hidden="true" />}
        {partial.translation && <p className="bubble-translation streaming-translation" lang={contentLanguageTag(partial.target_language)}>{partial.translation}<span className="streaming-ellipsis" aria-hidden="true">…</span></p>}
      </div>
    </div>
  ));
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
