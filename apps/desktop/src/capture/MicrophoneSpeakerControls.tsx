import { useTranslation } from "react-i18next";
import type { MicrophoneSpeakerControl } from "./useMicrophoneSpeakers";

export function MicrophoneSpeakerControls({ control }: { control: MicrophoneSpeakerControl }) {
  const { t } = useTranslation();
  const s = control.snapshot;
  const locked = control.busy || !control.connected || !s;
  const ready = !!s?.supported && s.running && !!s.generation;
  const status = !control.connected || !s ? "unavailable" : !s.running ? null : !s.supported ? "unsupported"
    : !s.generation ? "connecting" : "sessionOnly";
  const run = (action: Parameters<typeof control.execute>[0]) => { void control.execute(action).catch(() => {}); };
  return <fieldset className="player-history-languages microphone-speaker-controls" aria-busy={control.busy || undefined}>
    <legend>{t("live.microphoneSpeakers.title")}</legend>
    <label className="player-history-check">
      <input type="checkbox" checked={s?.enabled ?? false} disabled={locked}
        onChange={event => run({ set_microphone_filter: event.target.checked })} />
      <span>{t("live.microphoneSpeakers.enable")}</span>
    </label>
    {status && <small role="status">{t(`live.microphoneSpeakers.${status}`)}</small>}
    <div className="player-history-list">{s?.speakers.map(speaker => <div className="player-history-row" key={speaker.id}>
      <span className="player-history-name">{t("live.speakerNumber", { number: speaker.index + 1 })}
        <small>{t(`live.microphoneSpeakers.${speaker.decision}`)} · {t("live.microphoneSpeakers.lastActive", { time: new Date(speaker.last_active_at).toLocaleTimeString() })}</small>
      </span>
      {(["allow", "block"] as const).map(decision => <button key={decision} type="button" className="secondary-button"
        aria-pressed={speaker.decision === decision} disabled={locked || !ready || !s.enabled || speaker.decision === decision}
        onClick={() => run({ set_microphone_speaker: { generation: s.generation, revision: s.revision, id: speaker.id, decision } })}>
        {t(`live.microphoneSpeakers.${decision === "allow" ? "allowAction" : "blockAction"}`)}
      </button>)}
    </div>)}</div>
    {ready && !s?.speakers.length && <p className="player-history-empty">{t("live.microphoneSpeakers.empty")}</p>}
    {control.error && <p role="alert">{control.error}</p>}
  </fieldset>;
}
