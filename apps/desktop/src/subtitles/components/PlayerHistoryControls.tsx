import { useId, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ChevronDown, Info, MessageSquare, MessageSquareOff, Volume2, VolumeX, Plus, SlidersHorizontal, UsersRound, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { PlayerProximitySettings, PlayerViewPrioritySettings } from "../../capture/types";
import type { PlayerAudioStatus } from "../../core-client/types";
import type { SpeakerIdentity, Subtitle } from "../types";
import { useSpeakerIdentity } from "../speaker-state";
import { useHealthSelector } from "../../core-client/health-store";
import { isAudioTrack, type PlayerHistoryPreferences } from "../player-history";
import { useLanguageDetectorReady } from "../usePlayerHistoryPreferences";
import { collectPlayerSpeakers, playerLanguageOptions, applyPlayerVisibilityAction, sortPlayerSpeakersByDistance, type PlayerSourceAction, type PlayerVisibilityAction } from "../player-controls";
import type { PlayerSourceMode, PlayerSourceOverride } from "../../capture/types";
import { playerUserId } from "../../capture/player-selection";
import { localizedLanguageName } from "../../translation/translation-languages";
import { LanguagePicker } from "../../shared/ui/LanguagePicker";
import { useDismissibleLayer } from "../../shared/hooks/use-dismissible-layer";
import { useDropdownMenuMotion } from "../../shared/hooks/use-dropdown-menu-motion";
import { PreferenceToggle, RangeField } from "../../settings/SettingsControls";

export function PlayerHistoryControls({ subtitles, status, roster, useLiveHealth = false, enabled, proximityEnabled, selectedUserNames = {}, sourceOverrides = {}, sourceMode = "allowlist", proximity, viewPriority, defaultLanguages, preferences, onPreferences, onSource, onSourceMode, onProximityChange, onViewPriorityChange }: {
  subtitles: Subtitle[];
  status?: PlayerAudioStatus | null;
  roster?: PlayerAudioStatus["players"];
  useLiveHealth?: boolean;
  enabled: boolean;
  proximityEnabled?: boolean;
  selectedUserNames?: Record<string, string>;
  sourceOverrides?: Record<string, PlayerSourceOverride>;
  sourceMode?: PlayerSourceMode;
  proximity?: PlayerProximitySettings;
  viewPriority?: PlayerViewPrioritySettings;
  defaultLanguages: string[];
  preferences: PlayerHistoryPreferences;
  onPreferences: (apply: (value: PlayerHistoryPreferences) => PlayerHistoryPreferences) => void;
  onSource?: (action: PlayerSourceAction, speaker: SpeakerIdentity) => Promise<unknown>;
  onSourceMode?: (mode: PlayerSourceMode) => Promise<unknown> | void;
  onProximityChange?: (radius_m: number) => Promise<unknown> | void;
  onViewPriorityChange?: (enabled: boolean, horizontal_fov_deg: number) => Promise<unknown> | void;
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
  const close = () => {
    setOpen(false);
    if (rootRef.current?.contains(document.activeElement)) triggerRef.current?.focus({ preventScroll: true });
  };
  useDismissibleLayer(open, rootRef, close);
  const [busy, setBusy] = useState(false);
  const [query, setQuery] = useState("");
  const [group, setGroup] = useState<"current" | "history">("current");
  const livePlayerAudio = useHealthSelector((health) => (useLiveHealth && panelMounted ? health?.player_audio ?? null : null));
  const liveRoster = useHealthSelector((health) => (useLiveHealth && panelMounted ? health?.network_players : undefined));
  const effectiveStatus = status !== undefined ? status : livePlayerAudio;
  const effectiveRoster = roster !== undefined ? roster : liveRoster;
  const current = useMemo(
    () => panelMounted ? (effectiveRoster ?? (enabled ? effectiveStatus?.players ?? [] : [])) : [],
    [panelMounted, effectiveRoster, enabled, effectiveStatus?.players],
  );
  const currentIds = useMemo(() => current.map((player) => player.speaker.id), [current]);
  const savedUserIds = useMemo(() => Object.keys(sourceOverrides), [sourceOverrides]);
  const playerDistances = useMemo(() => new Map(current.map(player => [player.speaker.id, player.distance_m])), [current]);
  const speakers = useMemo(() => panelMounted ? sortPlayerSpeakersByDistance(
    collectPlayerSpeakers(subtitles, current.map(p => p.speaker), savedUserIds, selectedUserNames),
    playerDistances,
  ) : [], [panelMounted, subtitles, current, savedUserIds, selectedUserNames, playerDistances]);
  const currentSpeakerIds = new Set(currentIds);
  const currentSpeakers = speakers.filter((speaker) => currentSpeakerIds.has(speaker.id));
  const historySpeakers = speakers.filter((speaker) => !currentSpeakerIds.has(speaker.id));
  const visibleSpeakers = group === "current" ? currentSpeakers : historySpeakers;
  const hiddenUserIds = useMemo(() => new Set(speakers
    .filter((speaker) => preferences.hiddenTracks.includes(speaker.id))
    .map(playerUserId)
    .filter((id): id is string => id !== null)), [speakers, preferences.hiddenTracks]);
  const languages = preferences.hiddenSourceLanguages ?? [];
  const distanceEnabled = proximityEnabled ?? enabled;
  const displayedRadius = proximity?.enabled ? proximity.radius_m : 0;
  const ruleChips = [
    proximity && {
      key: "proximity",
      label: t("live.players.proximityRadius"),
      value: displayedRadius > 0 ? `${displayedRadius.toFixed(1)} m` : t("live.players.proximityOff"),
      active: displayedRadius > 0,
    },
    viewPriority && {
      key: "view",
      label: t("live.players.viewAngle"),
      value: viewPriority.enabled ? `${Math.round(viewPriority.horizontal_fov_deg)}°` : t("live.players.proximityOff"),
      active: viewPriority.enabled,
    },
  ].filter((item): item is { key: string; label: string; value: string; active: boolean } => Boolean(item));
  const languageDetectorReady = useLanguageDetectorReady();
  const languageCodes = useMemo(
    () => panelMounted
      ? playerLanguageOptions(subtitles, defaultLanguages, preferences.hiddenSourceLanguages).filter(language => !languages.includes(language))
      : [],
    [panelMounted, subtitles, defaultLanguages, preferences.hiddenSourceLanguages, languages, languageDetectorReady],
  );
  const languageName = (language: string) => localizedLanguageName(language, i18n.resolvedLanguage ?? "en-US");
  const display = (action: PlayerVisibilityAction) => onPreferences(value => applyPlayerVisibilityAction(value, action, defaultLanguages));
  const setLanguageHidden = (language: string, hidden: boolean) => display({ set_source_language_hidden: { language, hidden } });
  const setSource = async (speaker: SpeakerIdentity, source: "vrchat_chatbox" | "speaker", disabled: boolean) => {
    const userId = playerUserId(speaker);
    if (!onSource || !userId) return;
    setBusy(true);
    try { await onSource({ set_user_source_disabled: { user_id: userId, source, disabled } }, speaker); }
    catch { /* The shared settings error banner reports persistence failures. */ }
    finally { setBusy(false); }
  };
  const emptyState = !visibleSpeakers.length
    ? splitEmptyState(t(group === "current" ? "live.players.currentEmpty" : "live.players.historyEmpty"))
    : null;
  const namePendingNote = group === "current" && current.some((player) => !player.speaker.display_name)
    ? splitEmptyState(t("live.players.namePending"))
    : null;
  return <div className="player-history-controls" ref={rootRef} onPointerDown={(event) => {
    // Portaled child menus remain inside this panel's dismissal boundary.
    if (!event.currentTarget.contains(event.target as Node)) event.stopPropagation();
  }}>
    <button type="button" className="player-history-trigger" ref={triggerRef}
      data-tooltip={t("live.players.title")} aria-label={t("live.players.title")}
      aria-expanded={open} aria-controls={panelId} onClick={() => {
        if (!open) setGroup("current");
        setOpen(!open);
      }}>
      <UsersRound size={18} aria-hidden="true" />
    </button>
    {panelMounted && <section className="player-history-panel" ref={menuRef} id={panelId}
      aria-label={t("live.players.title")} aria-hidden={!open} inert={!open}>
    <div className="player-history-body">
      {ruleChips.length > 0 && <details className="player-history-rules">
        <summary className="player-history-rules-summary">
          <span className="player-history-rules-heading">
            <SlidersHorizontal size={14} aria-hidden="true" />
            <span>{t("live.players.rulesTab")}</span>
          </span>
          <span className="player-history-rules-values">
            {ruleChips.map((chip) => <span key={chip.key} className={`player-history-rule-chip${chip.active ? " is-active" : ""}`}>
              <span>{chip.label}</span>
              <strong>{chip.value}</strong>
            </span>)}
          </span>
          <ChevronDown className="player-history-rules-chevron" size={15} aria-hidden="true" />
        </summary>
        <div className="player-history-rules-content">
          {proximity && <div className="player-history-proximity" role="group" aria-label={t("live.players.proximityRadius")}>
            <RangeField label={t("live.players.proximityRadius")} value={displayedRadius}
              min={0} max={10} step={0.1} disabled={!distanceEnabled || !onProximityChange}
              formatValue={(radius) => radius > 0 ? `${radius.toFixed(1)} m` : t("live.players.proximityOff")}
              onCommit={(radius_m) => void onProximityChange?.(radius_m)} />
          </div>}
          {viewPriority && <div className="player-history-proximity" role="group" aria-label={t("live.players.viewPriority")}>
            <PreferenceToggle title={t("live.players.viewPriority")}
              checked={viewPriority.enabled} disabled={!enabled || !onViewPriorityChange}
              onChange={(checked) => void onViewPriorityChange?.(checked, viewPriority.horizontal_fov_deg)} />
            <RangeField label={t("live.players.viewAngle")} value={viewPriority.horizontal_fov_deg}
              min={30} max={180} step={1} disabled={!enabled || !viewPriority.enabled || !onViewPriorityChange}
              formatValue={(degrees) => `${degrees}°`}
              onCommit={(horizontal_fov_deg) => void onViewPriorityChange?.(true, horizontal_fov_deg)} />
            {effectiveStatus?.proximity?.view_enabled && !effectiveStatus.proximity.local_view && <div className="player-history-note" role="status">
              <Info size={13} aria-hidden="true" />
              <span>{t("live.players.viewWaiting")}</span>
            </div>}
          </div>}
        </div>
      </details>}
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
      <div className="player-history-actions player-history-list-toolbar">
        <input className="player-history-search" type="search" value={query} placeholder={t("live.players.search")} aria-label={t("live.players.search")} onChange={(event) => setQuery(event.target.value)} />
        <button type="button" className="secondary-button player-history-show-all" disabled={!preferences.hiddenTracks.length}
          data-tooltip={t("live.players.showAll")} aria-label={t("live.players.showAll")}
          onClick={() => display({ show_all_tracks: null })}><X size={15} aria-hidden="true" /></button>
        <div className="player-history-groups" role="group" aria-label={t("live.players.groups")}>
          {(["current", "history"] as const).map((kind) => <button key={kind} type="button" data-player-group={kind}
            className={group === kind ? "is-active" : ""} aria-pressed={group === kind} onClick={() => setGroup(kind)}>
            {t(kind === "current" ? "live.players.current" : "live.players.historySaved")}
            <small aria-hidden="true">{kind === "current" ? currentSpeakers.length : historySpeakers.length}</small>
          </button>)}
        </div>
      </div>
      <div className="player-history-source-mode" role="group" aria-label={t("live.players.sourceMode")}>
        {(["everyone", "allowlist"] as const).map((mode) => <button key={mode} type="button" data-source-mode={mode}
          className={sourceMode === mode ? "is-active" : ""} aria-pressed={sourceMode === mode}
          disabled={!onSourceMode} onClick={() => void onSourceMode?.(mode)}>
          {t(mode === "everyone" ? "live.players.sourceModeEveryone" : "live.players.sourceModeAllowlist")}
        </button>)}
      </div>
      <div className="player-history-list">{visibleSpeakers.map((speaker) => <PlayerRow key={speaker.id} speaker={speaker} query={query}
        current={currentIds.includes(speaker.id)}
        distance={playerDistances.get(speaker.id)}
        busy={busy}
        hidden={preferences.hiddenTracks.includes(speaker.id)}
        hiddenUserIds={hiddenUserIds}
        sourceEnabled={Boolean(onSource)}
        sourceOverrides={sourceOverrides}
        sourceMode={sourceMode}
        onSource={(source, disabled, resolved) => void setSource(resolved, source, disabled)} />)}
        {emptyState && <div className="player-history-empty" role="status">
          <span className="player-history-empty-icon" aria-hidden="true"><UsersRound size={16} /></span>
          <strong>{emptyState.title}</strong>
          {emptyState.hint && <small>{emptyState.hint}</small>}
        </div>}
      </div>
      {namePendingNote && <div className="player-history-note" role="status">
        <Info size={14} aria-hidden="true" />
        <div>
          <strong>{namePendingNote.title}</strong>
          {namePendingNote.hint && <small>{namePendingNote.hint}</small>}
        </div>
      </div>}
    </div>
    </section>}
  </div>;
}

function splitEmptyState(text: string): { title: string; hint?: string } {
  const match = text.match(/^(.*?)(?:。|\.\s+)(.+)$/);
  if (match) return { title: match[1].trim(), hint: match[2].trim() };
  return { title: text.replace(/[。.]$/, "").trim() };
}

function sourceDisabled(overrides: Record<string, PlayerSourceOverride>, mode: PlayerSourceMode, speaker: SpeakerIdentity, source: "chatbox" | "voice"): boolean {
  const userId = playerUserId(speaker);
  const rule = userId ? overrides[userId] : undefined;
  // A missing rule inherits the mode: off until enabled, or on until disabled.
  if (!rule) return mode === "allowlist";
  return source === "chatbox" ? rule.chatbox_disabled : rule.voice_disabled;
}

function PlayerRow({ speaker: identity, query, current, distance, busy, hidden, hiddenUserIds, sourceEnabled, sourceOverrides, sourceMode, onSource }: {
  speaker: SpeakerIdentity; query: string; current: boolean; distance?: number; busy: boolean; hidden: boolean;
  hiddenUserIds: ReadonlySet<string>; sourceEnabled: boolean;
  sourceOverrides: Record<string, PlayerSourceOverride>; sourceMode: PlayerSourceMode;
  onSource: (source: "vrchat_chatbox" | "speaker", disabled: boolean, speaker: SpeakerIdentity) => void;
}) {
  const { t } = useTranslation();
  const speaker = useSpeakerIdentity(identity)!;
  const userId = playerUserId(speaker);
  const chatboxDisabled = sourceDisabled(sourceOverrides, sourceMode, speaker, "chatbox");
  const voiceDisabled = sourceDisabled(sourceOverrides, sourceMode, speaker, "voice");
  const saved = speaker.id.startsWith("saved-user:");
  const voiceLocked = !!userId && hiddenUserIds.has(userId);
  const label = speaker.id === "other:unidentified" ? t("live.players.unidentified") : speaker.display_name
    || userId || t(isAudioTrack(speaker) || current ? "live.trackNumber" : "live.speakerNumber", { number:speaker.index + 1 });
  const source = (kind: "vrchat_chatbox" | "speaker", disabled: boolean, icon: ReactNode) => {
    const locked = kind === "speaker" && voiceLocked;
    const text = locked ? t("live.players.unhideToEnableVoice", { name: label })
      : t(`live.players.${disabled ? "enable" : "disable"}${kind === "vrchat_chatbox" ? "Chatbox" : "Voice"}`, { name: label });
    return <button type="button" className="secondary-button player-history-source"
      aria-pressed={disabled} data-tooltip={text} aria-label={text}
      disabled={busy || !sourceEnabled || !userId || locked}
      onClick={() => onSource(kind, !disabled, speaker)}>
      {icon}
    </button>;
  };
  if (query && !label.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())) return null;
  return <div className={`player-history-row ${hidden ? "is-hidden" : ""}`}>
    <span className="player-history-name">{label}<small>{current ? <>
      {t("live.players.current")} · <span className="player-history-distance">{distance === undefined
        ? t("live.players.distanceUnknown")
        : t("live.players.distance", { distance: distance.toFixed(1) })}</span>
    </> : t(saved ? "live.players.savedSelection" : "live.players.history")}</small>
      {current && !userId && <small className="player-history-pending-badge">{t("live.players.userIdPending")}</small>}</span>
    <span className="player-history-row-actions">
      {source("vrchat_chatbox", chatboxDisabled, chatboxDisabled ? <MessageSquareOff size={15} aria-hidden="true" /> : <MessageSquare size={15} aria-hidden="true" />)}
      {source("speaker", voiceDisabled, voiceDisabled ? <VolumeX size={15} aria-hidden="true" /> : <Volume2 size={15} aria-hidden="true" />)}
    </span>
  </div>;
}
