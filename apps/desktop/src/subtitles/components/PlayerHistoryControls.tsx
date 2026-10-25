import { useId, useMemo, useRef, useState } from "react";
import { Eye, EyeOff, UsersRound, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { PlayerAudioStatus } from "../../core-client/types";
import type { SpeakerIdentity, Subtitle } from "../types";
import { useSpeakerIdentity } from "../speaker-state";
import { historySpeakers, isAudioTrack, sourceLanguageKey, subtitleSourceLanguage, type PlayerHistoryPreferences } from "../player-history";
import { playerUserId } from "../../capture/player-selection";
import { localizedLanguageName, TRANSLATION_LANGUAGE_CODES } from "../../translation/translation-languages";
import { Select } from "../../settings/SettingsControls";
import { useDismissibleLayer } from "../../shared/hooks/use-dismissible-layer";

export function PlayerHistoryControls({ subtitles, status, enabled, selectedUserIds, selectedUserNames = {}, defaultLanguages, preferences, onPreferences, onSelection }: {
  subtitles: Subtitle[];
  status?: PlayerAudioStatus | null;
  enabled: boolean;
  selectedUserIds: string[] | null;
  selectedUserNames?: Record<string, string>;
  defaultLanguages: string[];
  preferences: PlayerHistoryPreferences;
  onPreferences: (value: PlayerHistoryPreferences) => void;
  onSelection: (value: string[] | null, names: Record<string, string>) => Promise<unknown>;
}) {
  const { t, i18n } = useTranslation();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelId = useId();
  const titleId = useId();
  const close = () => {
    setOpen(false);
    if (rootRef.current?.contains(document.activeElement)) triggerRef.current?.focus({ preventScroll: true });
  };
  useDismissibleLayer(open, rootRef, close);
  const [busy, setBusy] = useState(false);
  const [query, setQuery] = useState("");
  const current = enabled ? status?.players ?? [] : [];
  const currentIds = current.map((player) => player.speaker.id);
  const currentUsers = [...new Set(current.map((player) => playerUserId(player.speaker)).filter((id): id is string => id !== null))];
  const speakers = useMemo(() => {
    const known = new Map(historySpeakers(subtitles).map((speaker) => [speaker.id, speaker]));
    for (const player of status?.players ?? []) if (enabled) known.set(player.speaker.id, player.speaker);
    for (const user_id of selectedUserIds ?? []) {
      if (![...known.values()].some((speaker) => playerUserId(speaker) === user_id)) {
        known.set(`saved-user:${user_id}`, {id:`saved-user:${user_id}`, user_id, index:0, display_name:selectedUserNames[user_id]});
      }
    }
    return [...known.values()];
  }, [subtitles, status?.players, enabled, selectedUserIds, selectedUserNames]);
  const languageCodes = [...new Set([...defaultLanguages, ...(preferences.hiddenSourceLanguages ?? []),
    ...subtitles.filter((subtitle) => (subtitle.source ?? "speaker") === "speaker").map(subtitleSourceLanguage),
    ...TRANSLATION_LANGUAGE_CODES].map(sourceLanguageKey).filter((language): language is string => language !== null))];
  const languages = preferences.hiddenSourceLanguages ?? defaultLanguages;
  const languageName = (language: string) => localizedLanguageName(language, i18n.resolvedLanguage ?? "en-US");
  const select = async (value: string[] | null, updatedSpeaker?: SpeakerIdentity) => {
    setBusy(true);
    const labels = new Map(Object.entries(selectedUserNames));
    for (const speaker of speakers) {
      const id = playerUserId(speaker);
      if (id && speaker.display_name && !labels.has(id)) labels.set(id, speaker.display_name);
    }
    // Historical sentences may still carry an older name. Only currently
    // observed metadata can replace a saved label for the same account.
    for (const speaker of [...current.map((player) => player.speaker),
      ...(updatedSpeaker && currentIds.includes(updatedSpeaker.id) ? [updatedSpeaker] : [])]) {
      const id = playerUserId(speaker);
      if (id && speaker.display_name) labels.set(id, speaker.display_name);
    }
    // Only selected account IDs and their last known labels are persisted.
    const names = Object.fromEntries((value ?? []).flatMap((id) => labels.has(id) ? [[id, labels.get(id)!]] : []));
    try { await onSelection(value, names); } catch { /* The shared settings error banner reports persistence failures. */ }
    finally { setBusy(false); }
  };
  return <div className="player-history-controls" ref={rootRef}>
    <button type="button" className="player-history-trigger" ref={triggerRef}
      data-tooltip={t("live.players.title")} aria-label={t("live.players.title")}
      aria-expanded={open} aria-controls={panelId} onClick={() => setOpen(!open)}>
      <UsersRound size={18} aria-hidden="true" />
    </button>
    {open && <section className="player-history-panel" id={panelId} aria-labelledby={titleId}>
      <header className="player-history-heading">
        <strong id={titleId}>{t("live.players.title")}</strong>
        {preferences.hiddenTracks.length > 0 && <small>{t("live.players.hiddenCount", { count: preferences.hiddenTracks.length })}</small>}
        <button type="button" className="player-history-close" aria-label={t("common.close")} onClick={close}><X size={16} aria-hidden="true" /></button>
      </header>
    <div className="player-history-body">
      {enabled && <div className="player-history-processing">
        <Select label={t("live.players.process")} value={selectedUserIds === null ? "all" : "selected"} disabled={busy}
          options={[{value:"all",label:t("live.players.all")},{value:"selected",label:t("live.players.selected")}]}
          onChange={(value) => void select(value === "all" ? null : [])} />
        {selectedUserIds !== null && <div className="player-history-actions">
          <button type="button" className="secondary-button" disabled={busy || !currentUsers.length} onClick={() => void select([...new Set([...selectedUserIds, ...currentUsers])])}>{t("live.players.selectCurrent")}</button>
          <button type="button" className="secondary-button" disabled={busy || !selectedUserIds.length} onClick={() => void select([])}>{t("live.players.selectNone")}</button>
          {!currentUsers.some((id) => selectedUserIds.includes(id)) && <small role="status">{t("live.players.noneSelected")}</small>}
        </div>}
      </div>}
      <fieldset className="player-history-languages">
        <legend>{t("live.players.languages")}</legend>
        <label className="player-history-check"><input type="checkbox" checked={preferences.hiddenSourceLanguages === null}
          onChange={(event) => onPreferences({...preferences, hiddenSourceLanguages:event.target.checked ? null : [...defaultLanguages]})} />
          <span>{t("live.players.followDefault")}<small>{defaultLanguages.map(languageName).join(" · ") || t("live.players.noHiddenLanguages")}</small></span>
        </label>
        {preferences.hiddenSourceLanguages !== null && <div className="player-history-language-list">{languageCodes.map((language) => <label className="player-history-check" key={language}>
          <input type="checkbox" value={language} checked={languages.includes(language)} onChange={(event) => onPreferences({...preferences,
            hiddenSourceLanguages:event.target.checked ? [...languages, language] : languages.filter((value) => value !== language)})} />{languageName(language)}
        </label>)}</div>}
      </fieldset>
      <div className="player-history-actions">
        <input className="player-history-search" type="search" value={query} placeholder={t("live.players.search")} aria-label={t("live.players.search")} onChange={(event) => setQuery(event.target.value)} />
        <button type="button" className="secondary-button" disabled={!preferences.hiddenTracks.length}
          onClick={() => onPreferences({...preferences,hiddenTracks:[]})}>{t("live.players.showAll")}</button>
      </div>
      <div className="player-history-list">{speakers.map((speaker) => <PlayerRow key={speaker.id} speaker={speaker} query={query}
        current={currentIds.includes(speaker.id)} selectionEnabled={enabled && selectedUserIds !== null}
        selectedUserIds={selectedUserIds} busy={busy}
        hidden={preferences.hiddenTracks.includes(speaker.id)}
        onSelected={(selected, speaker) => {
          const userId = playerUserId(speaker)!;
          void select(selected ? [...new Set([...(selectedUserIds ?? []), userId])] : (selectedUserIds ?? []).filter((id) => id !== userId), speaker);
        }}
        onHidden={(hidden) => onPreferences({...preferences, hiddenTracks:hidden ? [...preferences.hiddenTracks, speaker.id] : preferences.hiddenTracks.filter((id) => id !== speaker.id)})} />)}
        {!speakers.length && <p className="player-history-empty">{t("live.players.empty")}</p>}
      </div>
      {current.some((player) => !player.speaker.display_name) && <small role="status">{t("live.players.namePending")}</small>}
    </div>
    </section>}
  </div>;
}

function PlayerRow({ speaker: identity, query, current, selectionEnabled, selectedUserIds, busy, hidden, onSelected, onHidden }: {
  speaker: SpeakerIdentity; query: string; current: boolean; selectionEnabled: boolean; selectedUserIds: string[] | null; busy: boolean; hidden: boolean;
  onSelected: (selected: boolean, speaker: SpeakerIdentity) => void; onHidden: (hidden: boolean) => void;
}) {
  const { t } = useTranslation();
  const speaker = useSpeakerIdentity(identity)!;
  const userId = playerUserId(speaker);
  const saved = speaker.id.startsWith("saved-user:");
  const selected = selectedUserIds === null || !!userId && selectedUserIds.includes(userId);
  const label = speaker.id === "other:unidentified" ? t("live.players.unidentified") : speaker.display_name
    || userId || t(isAudioTrack(speaker) || current ? "live.trackNumber" : "live.speakerNumber", { number:speaker.index + 1 });
  if (query && !label.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())) return null;
  return <div className={`player-history-row ${hidden ? "is-hidden" : ""}`}>
    <span className="player-history-name">{label}<small>{current ? t("live.players.current") : t(saved ? "live.players.savedSelection" : "live.players.history")}</small>
      {selectionEnabled && current && !userId && <small>{t("live.players.userIdPending")}</small>}</span>
    {selectionEnabled && (current || userId) && <label className="player-history-check"><input type="checkbox" checked={selected} disabled={busy || !userId}
      aria-label={t("live.players.translatePerson", { name:label })} onChange={(event) => { if (userId) onSelected(event.target.checked, speaker); }} />{t("live.players.translate")}</label>}
    {!saved && <button type="button" className="secondary-button player-history-visibility" aria-pressed={hidden}
      aria-label={t(hidden ? "live.players.showPerson" : "live.players.hidePerson", {name:label})} onClick={() => onHidden(!hidden)}>
      {hidden ? <EyeOff size={15} /> : <Eye size={15} />}<span>{t(hidden ? "live.players.show" : "live.players.hide")}</span>
    </button>}
  </div>;
}
