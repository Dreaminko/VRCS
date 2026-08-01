import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { CalendarDays, History, Languages, MessageSquare, Mic, Volume2 } from "lucide-react";

import { timestamp } from "../app-utils";
import type { ConnectionState, Health, LiveTranscription, Settings, Subtitle } from "../types";
import { DropdownField } from "./DropdownField";

type SubtitleSource = "speaker" | "microphone";

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
        <div><span>{t("status.label")}</span><strong>{health?.capture_running ? t("status.transcribing") : t("status.waiting")}</strong></div>
        <i aria-hidden="true" />
        <div><span>{t("status.engine")}</span><strong>{engineLabel(settings)}</strong></div>
      </div>
    </div>
  );
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

export function LiveView({ subtitles, partials, running, onSelect }: {
  subtitles: Subtitle[];
  partials: Partial<Record<LiveTranscription["source"], LiveTranscription>>;
  running: boolean;
  onSelect: (context: string) => Promise<void>;
}) {
  const { t } = useTranslation();
  const chronological = [...subtitles].reverse();
  return (
    <section className="conversation" aria-label={t("live.title")}>
      {chronological.length ? chronological.map((subtitle, index) => (
        <ChatBubble key={subtitle.id ?? `${subtitle.created_at}-${index}`} subtitle={subtitle} onSelect={onSelect} />
      )) : (
        <div className="empty-state"><MessageSquare size={22} /><p>{running ? t("live.listening") : t("live.startHint")}</p></div>
      )}
      {([partials.speaker, partials.microphone].filter(Boolean) as LiveTranscription[]).map((partial) => (
        <div className={`message-group source-${partial.source} streaming-message`} key={`${partial.source}-${partial.utterance_id}`}>
          <div className="bubble">{partial.text}<span className="streaming-ellipsis" aria-hidden="true">…</span></div>
        </div>
      ))}
      {running && !partials.speaker && !partials.microphone && <div className="message-group source-speaker streaming-message"><div className="bubble">{t("live.transcribing")}<span className="streaming-ellipsis" aria-hidden="true">…</span></div></div>}
    </section>
  );
}

function ChatBubble({ subtitle, onSelect }: { subtitle: Subtitle; onSelect: (context: string) => Promise<void> }) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const source: SubtitleSource = subtitle.source ?? "speaker";
  const mine = source === "microphone";
  return (
    <article className={`message-group source-${source}`}>
      <div className="message-meta">
        {!mine && <Volume2 size={14} />}
        {mine && <time>{timestamp(subtitle.created_at, locale)}</time>}
        <span>{mine ? t("live.microphoneMe") : t("live.speakerOther")}</span>
        {!mine && <time>{timestamp(subtitle.created_at, locale)}</time>}
        {mine && <Mic size={14} />}
      </div>
      <p className="bubble" onMouseUp={() => void onSelect(subtitle.text)}>{subtitle.text}</p>
    </article>
  );
}

export function HistoryView({ subtitles, onSelect }: { subtitles: Subtitle[]; onSelect: (context: string) => Promise<void> }) {
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
          <article key={subtitle.id ?? `${subtitle.created_at}-${index}`} onMouseUp={() => void onSelect(subtitle.text)}>
            <time>{timestamp(subtitle.created_at, locale)}</time>
            <p>{subtitle.text}</p>
            <span>{subtitle.language?.toUpperCase() ?? "—"}</span>
          </article>
        ))}</div>
      ) : <div className="empty-state"><History size={22} /><p>{t("history.empty")}</p></div>}
    </section>
  );
}
