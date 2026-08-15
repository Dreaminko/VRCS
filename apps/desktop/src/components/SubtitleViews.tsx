import { memo, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { CalendarDays, History, Languages, MessageSquare, Mic, Volume2 } from "lucide-react";

import { timestamp } from "../app-utils";
import { useTranslationPartial } from "../realtime-state";
import { contentLanguageTag } from "../ui-language";
import type { ConnectionState, Health, LiveTranscription, Settings, Subtitle } from "../types";
import { DropdownField } from "./DropdownField";

type SubtitleSource = NonNullable<Subtitle["source"]>;

export function TopStatus({ connection, health, settings }: {
  connection: ConnectionState;
  health: Health | null;
  settings: Settings | null;
}) {
  const { t } = useTranslation();
  const connectionLabel = t(`status.connection.${connection}`);
  return (
    <div className="top-status-row">
      <div className="status-summary" aria-label={t("status.summary")}>
        <div className={`core-summary connection-${connection}`}><span>Core</span><strong><i aria-hidden="true" />{connectionLabel}</strong></div>
        <i aria-hidden="true" />
        <div><span>{t("status.label")}</span><strong>{transcriptionStatusLabel(health, t)}</strong></div>
        <i aria-hidden="true" />
        <div className={health?.osc?.send_gate === "open" ? "mute-summary" : "mute-summary muted"}>
          <span>{t("status.vrchat")}</span>
          <strong>{vrchatSendStatusLabel(health, settings, t)}</strong>
        </div>
        <i aria-hidden="true" />
        <div><span>{t("status.engine")}</span><strong>{engineLabel(settings)}</strong></div>
      </div>
    </div>
  );
}

function transcriptionStatusLabel(health: Health | null, t: (key: string) => string): string {
  if (!health?.capture_requested) return t("status.waiting");
  if (health.microphone_capture_state === "paused_vrchat_muted") {
    return t("status.microphonePaused");
  }
  return t("status.transcribing");
}

function vrchatSendStatusLabel(
  health: Health | null,
  settings: Settings | null,
  t: (key: string) => string,
): string {
  if (!settings?.osc.enabled || health?.osc?.status === "disabled") return t("status.vrchatSendDisabled");
  if (!health?.osc) return t("status.vrchatSendChecking");
  if (health?.osc?.status === "error") return t("status.vrchatSendError");
  if (health.osc.send_gate === "blocked_vrchat_muted") {
    return t("status.pausedVrchatMuted");
  }
  if (health.osc.send_gate === "blocked_mute_unknown") return t("status.muteUnknown");
  return t("status.sendReady");
}

function engineLabel(settings: Settings | null): string {
  if (settings?.asr.backend === "qwen_realtime") return "Qwen3 ASR";
  if (settings?.asr.backend === "fun_asr_realtime") return "Fun-ASR";
  if (settings?.asr.backend === "openai_realtime") return "OpenAI Realtime";
  return `Whisper ${capitalize(settings?.asr.local.model ?? "small")}`;
}

function capitalize(value: string): string {
  return value.charAt(0).toUpperCase() + value.slice(1);
}

export const LiveView = memo(function LiveView({ subtitles, partials, running, onSelect, onTranslate, translatingSubtitleIds = [] }: {
  subtitles: Subtitle[];
  partials: Partial<Record<LiveTranscription["source"], LiveTranscription>>;
  running: boolean;
  onSelect: (context: string) => Promise<void>;
  onTranslate?: (subtitleId: number) => void;
  translatingSubtitleIds?: number[];
}) {
  const { t } = useTranslation();
  const chronological = useMemo(() => [...subtitles].reverse(), [subtitles]);
  return (
    <section className="conversation" aria-label={t("live.title")}>
      {chronological.length ? chronological.map((subtitle, index) => (
        <ChatBubble key={subtitle.id ?? `${subtitle.created_at}-${index}`} subtitle={subtitle} onSelect={onSelect} onTranslate={onTranslate} translating={subtitle.id !== null && translatingSubtitleIds.includes(subtitle.id)} />
      )) : (
        <div className="empty-state"><MessageSquare size={22} /><p>{running ? t("live.listening") : t("live.startHint")}</p></div>
      )}
      {([partials.speaker, partials.microphone].filter(Boolean) as LiveTranscription[]).map((partial) => (
        <div className={`message-group source-${partial.source} streaming-message`} key={`${partial.source}-${partial.utterance_id}`}>
          <div className="bubble" lang={contentLanguageTag(partial.language)}>{partial.text}<span className="streaming-ellipsis" aria-hidden="true">…</span></div>
        </div>
      ))}
      {running && !partials.speaker && !partials.microphone && <div className="message-group source-speaker streaming-message"><div className="bubble">{t("live.transcribing")}<span className="streaming-ellipsis" aria-hidden="true">…</span></div></div>}
    </section>
  );
});

const ChatBubble = memo(function ChatBubble({ subtitle, onSelect, onTranslate, translating }: {
  subtitle: Subtitle;
  onSelect: (context: string) => Promise<void>;
  onTranslate?: (subtitleId: number) => void;
  translating?: boolean;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const source: SubtitleSource = subtitle.source ?? "speaker";
  const mine = source !== "speaker";
  const translationPartial = useTranslationPartial(subtitle.id);
  const completedTranslation = subtitle.translations.at(-1);
  const visibleTranslation = translationPartial ?? subtitle.translation_partial ?? completedTranslation;
  return (
    <article className={`message-group source-${source}`}>
      <div className="message-meta">
        {!mine && <Volume2 size={14} />}
        {mine && <time>{timestamp(subtitle.created_at, locale)}</time>}
        <span>{source === "chatbox" ? t("chatbox.title") : mine ? t("live.microphoneMe") : t("live.speakerOther")}</span>
        {!mine && <time>{timestamp(subtitle.created_at, locale)}</time>}
        {source === "microphone" && <Mic size={14} />}
        {source === "chatbox" && <MessageSquare size={14} />}
      </div>
      <div className="bubble">
        <p className="bubble-original" lang={contentLanguageTag(subtitle.language)} onMouseUp={() => void onSelect(subtitle.text)}>{subtitle.text}</p>
        {visibleTranslation && (
          <>
            <div className="bubble-translation-divider" aria-hidden="true" />
            <p className={`bubble-translation ${translationPartial || subtitle.translation_partial ? "streaming-translation" : ""}`} lang={contentLanguageTag(visibleTranslation.target_language)}>
              {visibleTranslation.text}
              {(translationPartial || subtitle.translation_partial) && <span className="streaming-ellipsis" aria-hidden="true">…</span>}
            </p>
          </>
        )}
      </div>
      {onTranslate && subtitle.id !== null && (
        <button className="translation-action" type="button" disabled={translating} onClick={() => onTranslate(subtitle.id!)}>
          <Languages size={13} />{t(translating ? "translation.translating" : subtitle.translations.length ? "translation.retry" : "translation.action")}
        </button>
      )}
    </article>
  );
});

const HistoryRow = memo(function HistoryRow({ subtitle, locale, onSelect, onTranslate, translating }: {
  subtitle: Subtitle;
  locale: string;
  onSelect: (context: string) => Promise<void>;
  onTranslate?: (subtitleId: number) => void;
  translating: boolean;
}) {
  const { t } = useTranslation();
  const translationPartial = useTranslationPartial(subtitle.id);
  const visibleTranslation = translationPartial ?? subtitle.translation_partial ?? subtitle.translations.at(-1);
  return (
    <article onMouseUp={() => void onSelect(subtitle.text)}>
      <time>{timestamp(subtitle.created_at, locale)}</time>
      <p lang={contentLanguageTag(subtitle.language)}>{subtitle.text}</p>
      {visibleTranslation && (
        <p className={translationPartial || subtitle.translation_partial ? "history-translation streaming-translation" : "history-translation"}>
          {visibleTranslation.text}
          {(translationPartial || subtitle.translation_partial) && <span className="streaming-ellipsis" aria-hidden="true">…</span>}
        </p>
      )}
      <span>{subtitle.language?.toUpperCase() ?? "—"}</span>
      {onTranslate && subtitle.id !== null && (
        <button className="translation-action" type="button" disabled={translating} onClick={() => onTranslate(subtitle.id!)}>
          <Languages size={13} />{t(translating ? "translation.translating" : subtitle.translations.length ? "translation.retry" : "translation.action")}
        </button>
      )}
    </article>
  );
});

export const HistoryView = memo(function HistoryView({ subtitles, onSelect, onTranslate, translatingSubtitleIds = [], hasOlder, loadingOlder, onLoadOlder }: {
  subtitles: Subtitle[];
  onSelect: (context: string) => Promise<void>;
  onTranslate?: (subtitleId: number) => void;
  translatingSubtitleIds?: number[];
  hasOlder: boolean;
  loadingOlder: boolean;
  onLoadOlder: () => Promise<void>;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const [language, setLanguage] = useState("all");
  const [range, setRange] = useState("all");
  const filtered = useMemo(() => {
    const now = Date.now();
    return subtitles.filter((subtitle) => {
      if (language !== "all" && subtitle.language !== language) return false;
      if (range === "today" && now - new Date(subtitle.created_at).getTime() > 86_400_000) return false;
      if (range === "week" && now - new Date(subtitle.created_at).getTime() > 604_800_000) return false;
      return true;
    });
  }, [language, range, subtitles]);

  return (
    <section className="history-surface">
      <div className="history-toolbar">
        <div><h2>{t("history.title")}</h2><span>{t("history.recordCount", { count: filtered.length })}</span></div>
        <div className="history-filters">
          <DropdownField
            compact
            icon={<Languages size={15} />}
            label={t("history.language")}
            value={language}
            options={[
              { value: "all", label: t("languages.all") },
              { value: "ja", label: t("languages.japanese") },
              { value: "en", label: t("languages.english") },
              { value: "zh", label: t("languages.chinese") },
              { value: "ko", label: t("languages.korean") },
            ]}
            onChange={setLanguage}
          />
          <DropdownField
            compact
            icon={<CalendarDays size={15} />}
            label={t("history.dateRange")}
            value={range}
            options={[
              { value: "all", label: t("history.allTime") },
              { value: "today", label: t("date.today") },
              { value: "week", label: t("history.lastSevenDays") },
            ]}
            onChange={setRange}
          />
        </div>
      </div>
      {filtered.length ? (
        <div className="history-list">{filtered.map((subtitle, index) => (
          <HistoryRow
            key={subtitle.id ?? `${subtitle.created_at}-${index}`}
            subtitle={subtitle}
            locale={locale}
            onSelect={onSelect}
            onTranslate={onTranslate}
            translating={subtitle.id !== null && translatingSubtitleIds.includes(subtitle.id)}
          />
        ))}</div>
      ) : <div className="empty-state"><History size={22} /><p>{t("history.empty")}</p></div>}
      {hasOlder && (
        <div className="history-load-older">
          <button className="secondary-button" type="button" disabled={loadingOlder} onClick={() => void onLoadOlder()}>
            {t(loadingOlder ? "history.loadingEarlier" : "history.loadEarlier")}
          </button>
        </div>
      )}
    </section>
  );
});
