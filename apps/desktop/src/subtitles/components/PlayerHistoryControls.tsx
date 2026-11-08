import { useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Eye, EyeOff, Plus, UsersRound, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { PlayerProximitySettings } from "../../capture/types";
import type { PlayerAudioStatus } from "../../core-client/types";
import type { SpeakerIdentity, Subtitle } from "../types";
import { useSpeakerIdentity } from "../speaker-state";
import { isAudioTrack, type PlayerHistoryPreferences } from "../player-history";
import { collectPlayerSpeakers, playerLanguageOptions, applyPlayerVisibilityAction, sortPlayerSpeakersByDistance, type PlayerSelectionAction, type PlayerVisibilityAction } from "../player-controls";
import { playerUserId } from "../../capture/player-selection";
import { localizedLanguageName } from "../../translation/translation-languages";
import { LanguagePicker } from "../../shared/ui/LanguagePicker";
import { useDismissibleLayer } from "../../shared/hooks/use-dismissible-layer";
import { useDropdownMenuMotion } from "../../shared/hooks/use-dropdown-menu-motion";
import { MicrophoneSpeakerControls } from "../../capture/MicrophoneSpeakerControls";
import type { MicrophoneSpeakerControl } from "../../capture/useMicrophoneSpeakers";
import { PreferenceToggle, RangeField } from "../../settings/SettingsControls";

export function PlayerHistoryControls({ subtitles, status, enabled, selectedUserIds, selectedUserNames = {}, proximity, defaultLanguages, preferences, onPreferences, onSelection, onProximityChange, microphone }: {
  microphone?: MicrophoneSpeakerControl;
  subtitles: Subtitle[];
  status?: PlayerAudioStatus | null;
  enabled: boolean;
  selectedUserIds: string[] | null;
  selectedUserNames?: Record<string, string>;
  proximity?: PlayerProximitySettings;
  defaultLanguages: string[];
  preferences: PlayerHistoryPreferences;
  onPreferences: (apply: (value: PlayerHistoryPreferences) => PlayerHistoryPreferences) => void;
  onSelection: (action: PlayerSelectionAction, speakers: readonly SpeakerIdentity[], current: readonly SpeakerIdentity[]) => Promise<unknown>;
  onProximityChange?: (proximity: PlayerProximitySettings) => Promise<unknown> | void;
}) {
  const { t, i18n } = useTranslation();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const { menuRef, mounted: panelMounted } = useDropdownMenuMotion(open);
  useLayoutEffect(() => {
    const panel = menuRef.current;
    const trigger = triggerRef.current;
    const container = panel?.offsetParent;
    if (!panel || !trigger || !container) return;

    const updateOrigin = () => {
      // Both controls share an offset parent. Layout coordinates ignore zoom and active transforms.
      const x = trigger.offsetLeft + trigger.offsetWidth / 2 - panel.offsetLeft;
      const y = trigger.offsetTop + trigger.offsetHeight / 2 - panel.offsetTop;
      panel.style.transformOrigin = `${x}px ${y}px`;
    };
    updateOrigin();
    const observer = new ResizeObserver(updateOrigin);
    for (const element of [panel, container, ...container.children]) observer.observe(element);
    return () => observer.disconnect();
  }, [menuRef, panelMounted]);
  const panelId = useId();
  const titleId = useId();
  const close = () => {
    setOpen(false);
    if (rootRef.current?.contains(document.activeElement)) triggerRef.current?.focus({ preventScroll: true });
  };
  useDismissibleLayer(open, rootRef, close);
  const [busy, setBusy] = useState(false);
  const processingMode = selectedUserIds === null ? "all" : "selected";
  const [query, setQuery] = useState("");
  const current = enabled ? status?.players ?? [] : [];
  const currentIds = current.map((player) => player.speaker.id);
  const currentUsers = [...new Set(current.map((player) => playerUserId(player.speaker)).filter((id): id is string => id !== null))];
  const playerDistances = useMemo(() => new Map(current.map(player => [player.speaker.id, player.distance_m])), [current]);
  const speakers = useMemo(() => sortPlayerSpeakersByDistance(
    collectPlayerSpeakers(subtitles, enabled ? (status?.players ?? []).map(p => p.speaker) : [], selectedUserIds, selectedUserNames),
    playerDistances,
  ), [subtitles, status?.players, enabled, selectedUserIds, selectedUserNames, playerDistances]);
  const languages = preferences.hiddenSourceLanguages ?? [];
  const languageCodes = playerLanguageOptions(subtitles, defaultLanguages, preferences.hiddenSourceLanguages).filter(language => !languages.includes(language));
  const languageName = (language: string) => localizedLanguageName(language, i18n.resolvedLanguage ?? "en-US");
  const display = (action: PlayerVisibilityAction) => onPreferences(value => applyPlayerVisibilityAction(value, action, defaultLanguages));
  const setLanguageHidden = (language: string, hidden: boolean) => display({ set_source_language_hidden: { language, hidden } });
  const select = async (action: PlayerSelectionAction, updatedSpeaker?: SpeakerIdentity) => {
    setBusy(true);
    const known = updatedSpeaker ? speakers.map(s => s.id === updatedSpeaker.id ? updatedSpeaker : s) : speakers;
    const observed = current.map(p => updatedSpeaker?.id === p.speaker.id ? updatedSpeaker : p.speaker);
    try { await onSelection(action, known, observed); } catch { /* The shared settings error banner reports persistence failures. */ }
    finally { setBusy(false); }
  };
  return <div className="player-history-controls" ref={rootRef} onPointerDown={(event) => {
    // Portaled child menus remain inside this panel's dismissal boundary.
    if (!event.currentTarget.contains(event.target as Node)) event.stopPropagation();
  }}>
    <button type="button" className="player-history-trigger" ref={triggerRef}
      data-tooltip={t("live.players.title")} aria-label={t("live.players.title")}
      aria-expanded={open} aria-controls={panelId} onClick={() => setOpen(!open)}>
      <UsersRound size={18} aria-hidden="true" />
    </button>
    {panelMounted && <section className="player-history-panel" ref={menuRef} id={panelId}
      aria-labelledby={titleId} aria-hidden={!open} inert={!open}>
      <header className="player-history-heading">
        <strong id={titleId}>{t("live.players.title")}</strong>
        {preferences.hiddenTracks.length > 0 && <small>{t("live.players.hiddenCount", { count: preferences.hiddenTracks.length })}</small>}
        <button type="button" className="player-history-close" aria-label={t("common.close")} onClick={close}><X size={16} aria-hidden="true" /></button>
      </header>
    <div className="player-history-body">
      {microphone && <MicrophoneSpeakerControls control={microphone} />}
      {enabled && <div className="player-history-processing">
        <div className="field">
          <span>{t("live.players.process")}</span>
          <div className="theme-toggle-group" role="group" aria-label={t("live.players.process")}>
            {(["all", "selected"] as const).map((mode) => <button key={mode} type="button"
              aria-pressed={processingMode === mode} className={processingMode === mode ? "active" : ""} disabled={busy}
              onClick={() => { if (processingMode !== mode) void select({ set_processing_mode: mode }); }}>
              {t(`live.players.${mode}`)}
            </button>)}
          </div>
        </div>
        {selectedUserIds !== null && <div className="player-history-actions">
          <button type="button" className="secondary-button" disabled={busy || !currentUsers.length} onClick={() => void select({ select_current: null })}>{t("live.players.selectCurrent")}</button>
          <button type="button" className="secondary-button" disabled={busy || !selectedUserIds.length} onClick={() => void select({ clear_selection: null })}>{t("live.players.selectNone")}</button>
          {!currentUsers.some((id) => selectedUserIds.includes(id)) && <small role="status">{t("live.players.noneSelected")}</small>}
        </div>}
      </div>}
      {proximity && <div className="player-history-proximity" role="group" aria-label={t("live.players.proximityEnable")}>
        <PreferenceToggle title={t("live.players.proximityEnable")} checked={proximity.enabled}
          disabled={!enabled || !onProximityChange}
          onChange={(enabled) => void onProximityChange?.({ ...proximity, enabled })} />
        <RangeField label={t("live.players.proximityRadius")} value={proximity.radius_m}
          min={1} max={10} step={0.1} disabled={!enabled || !proximity.enabled || !onProximityChange}
          formatValue={(radius) => `${radius.toFixed(1)} m`}
          onCommit={(radius_m) => void onProximityChange?.({ ...proximity, radius_m })} />
      </div>}
      <fieldset className="player-history-languages">
        <legend>{t("live.players.languages")}</legend>
        <div className="player-history-language-list">{languages.map((language) => <button type="button" className="language-chip player-history-language" key={language} value={language}
          aria-label={t("live.players.removeHiddenLanguage", { language: languageName(language) })} onClick={() => setLanguageHidden(language, false)}>
          {languageName(language)}<X size={13} aria-hidden="true" />
        </button>)}{!languages.length && <small>{t("live.players.noHiddenLanguages")}</small>}
          <span className="player-history-language-add">
            <LanguagePicker label={t("live.players.addHiddenLanguage")} value="" languageCodes={languageCodes}
              icon={<Plus size={14} aria-hidden="true" />} iconOnly compact floating disabled={!open || !languageCodes.length}
              onChange={(language) => setLanguageHidden(language, true)} />
          </span>
        </div>
      </fieldset>
      <div className="player-history-actions">
        <input className="player-history-search" type="search" value={query} placeholder={t("live.players.search")} aria-label={t("live.players.search")} onChange={(event) => setQuery(event.target.value)} />
        <button type="button" className="secondary-button" disabled={!preferences.hiddenTracks.length}
          onClick={() => display({ show_all_tracks: null })}>{t("live.players.showAll")}</button>
      </div>
      <div className="player-history-list">{speakers.map((speaker) => <PlayerRow key={speaker.id} speaker={speaker} query={query}
        current={currentIds.includes(speaker.id)} selectionEnabled={enabled && selectedUserIds !== null}
        distance={playerDistances.get(speaker.id)}
        selectedUserIds={selectedUserIds} busy={busy}
        hidden={preferences.hiddenTracks.includes(speaker.id)}
        onSelected={(selected, speaker) => {
          const userId = playerUserId(speaker)!;
          void select({ set_user_selected: { user_id: userId, selected } }, speaker);
        }}
        onHidden={(hidden) => display({ set_track_hidden: { track_id: speaker.id, hidden } })} />)}
        {!speakers.length && <p className="player-history-empty">{t("live.players.empty")}</p>}
      </div>
      {current.some((player) => !player.speaker.display_name) && <small role="status">{t("live.players.namePending")}</small>}
    </div>
    </section>}
  </div>;
}

function PlayerRow({ speaker: identity, query, current, distance, selectionEnabled, selectedUserIds, busy, hidden, onSelected, onHidden }: {
  speaker: SpeakerIdentity; query: string; current: boolean; distance?: number; selectionEnabled: boolean; selectedUserIds: string[] | null; busy: boolean; hidden: boolean;
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
    <span className="player-history-name">{label}<small>{current ? <>
      {t("live.players.current")} · <span className="player-history-distance">{distance === undefined
        ? t("live.players.distanceUnknown")
        : t("live.players.distance", { distance: distance.toFixed(1) })}</span>
    </> : t(saved ? "live.players.savedSelection" : "live.players.history")}</small>
      {selectionEnabled && current && !userId && <small>{t("live.players.userIdPending")}</small>}</span>
    {selectionEnabled && (current || userId) && <label className="player-history-check"><input type="checkbox" checked={selected} disabled={busy || !userId}
      aria-label={t("live.players.translatePerson", { name:label })} onChange={(event) => { if (userId) onSelected(event.target.checked, speaker); }} />{t("live.players.translate")}</label>}
    {!saved && <button type="button" className="secondary-button player-history-visibility" aria-pressed={hidden}
      aria-label={t(hidden ? "live.players.showPerson" : "live.players.hidePerson", {name:label})} onClick={() => onHidden(!hidden)}>
      {hidden ? <EyeOff size={15} /> : <Eye size={15} />}<span>{t(hidden ? "live.players.show" : "live.players.hide")}</span>
    </button>}
  </div>;
}
