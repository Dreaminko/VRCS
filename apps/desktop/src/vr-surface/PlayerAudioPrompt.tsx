import { useLayoutEffect, useRef } from "react";
import { ChevronLeft, ChevronRight, FlaskConical } from "lucide-react";
import type { PlayerAudioPrompt as Prompt } from "../capture/player-audio-vr";

export function PlayerAudioPrompt({ notice, hovered, pressed, dashboard = false }: {
  notice: Prompt; hovered: number | null; pressed: number | null; dashboard?: boolean;
}) {
  const viewport = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { if (viewport.current) viewport.current.scrollTop = 0; }, [notice.request_id]);
  const button = (id: number, action: "cancel" | "confirm" | "previous" | "next", label: string, disabled = false) =>
    <button type="button" tabIndex={-1} data-vr-region={id} disabled={disabled} aria-label={label}
      data-vr-command={dashboard ? JSON.stringify({ type: "player_audio", value: { request_id: notice.request_id, action } }) : undefined}
      className={`vr-player-audio-button${action === "confirm" ? " primary" : ""}${hovered === id ? " hovered" : ""}${pressed === id ? " pressed" : ""}`}>
      {action === "previous" ? <ChevronLeft /> : action === "next" ? <ChevronRight /> : label}
    </button>;
  return <section className="vr-player-audio-panel vr-surface-panel" data-vr-panel={!dashboard || undefined} role="dialog" aria-modal="true" aria-label={notice.title}>
    <header><FlaskConical /><h1>{notice.title}</h1></header>
    <div className="vr-player-audio-scroll-hint">{notice.scroll_hint}</div>
    <div className="vr-player-audio-viewport" data-vr-page-viewport ref={viewport}>
      <div data-vr-page-content>
        <p>{notice.intro}</p>
        {notice.sections.map((section, index) => <section key={index}>
          <h2>{section.title}</h2>{section.paragraphs.map((text, index) => <p key={index}>{text}</p>)}
        </section>)}
      </div>
    </div>
    <footer>
      <div className="vr-player-audio-reading-status" role="status">{notice.reading_status}</div>
      <nav>{button(2, "previous", notice.previous)}{button(3, "next", notice.next)}<span />
        {button(0, "cancel", notice.cancel, notice.remaining_seconds > 0)}
        {button(1, "confirm", notice.confirm, notice.remaining_seconds > 0)}</nav>
    </footer>
  </section>;
}
