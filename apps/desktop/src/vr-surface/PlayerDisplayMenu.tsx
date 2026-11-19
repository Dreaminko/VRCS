import { useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { Check, ChevronLeft, ChevronRight, Eye, Languages, MessageSquare, MessageSquareOff, SlidersHorizontal, UsersRound, Volume2, VolumeX, X } from "lucide-react";
import type { PlayerDisplayView, SliderCommand, WristMenuCommand, WristMenuUi } from "../vr-controls/player-display-protocol";
import type { SurfaceState } from "./protocol";

export const DEFAULT_WRIST_MENU_UI: WristMenuUi = { page: "home", group: "current", player_page: 0, language_page: 0 };

const PROXIMITY_SLIDER = { action: "proximity_radius", min: 0, max: 10, step: 0.1 } as const;
const VIEW_ANGLE_SLIDER = { action: "view_horizontal_fov", min: 30, max: 180, step: 5 } as const;

const unavailableView: PlayerDisplayView = {
  available: false, processing_available: false, player_mode: false, source_mode: "allowlist",
  players: [], languages: [], follow_default: false, default_languages: [], labels: {}, busy: false, error: null,
};

function MenuButton({ state, id, command, children, label, active = false, pressed, disabled = false, className = "" }: {
  state: SurfaceState; id: number; command: WristMenuCommand; children: ReactNode; label?: string;
  active?: boolean; pressed?: boolean; disabled?: boolean; className?: string;
}) {
  return <button type="button" tabIndex={-1} data-vr-region={id} data-vr-menu={JSON.stringify(command)}
    disabled={disabled} aria-label={label} aria-pressed={pressed ?? active}
    className={`vr-menu-button${active ? " active" : ""}${state.hovered === id ? " hovered" : ""}${state.pressed === id ? " pressed" : ""}${className ? ` ${className}` : ""}`}>
    {children}
  </button>;
}

function MenuSlider({ state, id, value, disabled, label, command, formatValue, scale, error }: {
  state: SurfaceState; id: number; value: number; disabled: boolean; label: string; command: SliderCommand;
  formatValue: (value: number) => string; scale: readonly [string, string]; error: string | null;
}) {
  const track = useRef<HTMLButtonElement>(null);
  const dragging = useRef(false);
  const errorAtPress = useRef<string | null>(null);
  const [preview, setPreview] = useState<number | null>(null);
  const pointer = state.pointer;
  useLayoutEffect(() => {
    const pressed = state.pressed === id;
    if (pressed && !dragging.current) errorAtPress.current = error;
    else if (error === null) errorAtPress.current = null;
    dragging.current = pressed;
    const element = track.current;
    const root = element?.closest<HTMLElement>(".vr-surface");
    if (disabled || !pressed || !pointer || !element || !root) return;
    const surface = root.getBoundingClientRect();
    const bounds = element.getBoundingClientRect();
    if (!surface.width || !bounds.width) return;
    const x = pointer[0] * surface.width - (bounds.left - surface.left);
    const fraction = Math.max(0, Math.min(1, x / bounds.width));
    const raw = command.min + fraction * (command.max - command.min);
    const quantized = command.min + Math.round((raw - command.min) / command.step) * command.step;
    setPreview(Math.round(Math.max(command.min, Math.min(command.max, quantized)) * 10) / 10);
  }, [command, disabled, error, id, pointer, state.pressed]);
  useLayoutEffect(() => {
    if (disabled || (preview !== null
      && (value === preview || (error !== null && error !== errorAtPress.current)))) setPreview(null);
  }, [disabled, error, preview, value]);
  const shown = preview ?? Math.max(command.min, Math.min(command.max, value));
  const text = formatValue(shown);
  const progress = (shown - command.min) / (command.max - command.min) * 100;
  return <div className="vr-menu-proximity-slider">
    <button type="button" tabIndex={-1} role="slider" ref={track} disabled={disabled}
      data-vr-region={id} data-vr-menu={JSON.stringify({ slider: command })}
      aria-label={label} aria-orientation="horizontal"
      aria-valuemin={command.min} aria-valuemax={command.max} aria-valuenow={shown}
      aria-valuetext={text}
      className="vr-menu-proximity-track"
      style={{ "--vr-proximity-progress": `${progress}%` } as CSSProperties}>
      <span className="vr-menu-proximity-line" aria-hidden="true">
        <span className="vr-menu-proximity-rail"><span className="vr-menu-proximity-fill" /></span>
        <span className="vr-menu-proximity-thumb" />
        <span className="vr-menu-proximity-scale"><span>{scale[0]}</span><span>{scale[1]}</span></span>
      </span>
    </button>
    <strong className="vr-menu-proximity-value">{text}</strong>
  </div>;
}

function pageSlice<T>(items: readonly T[], requested: number, size: number) {
  const count = Math.max(1, Math.ceil(items.length / size));
  const page = Math.max(0, Math.min(count - 1, Number.isFinite(requested) ? Math.floor(requested) : 0));
  return { page, count, items: items.slice(page * size, (page + 1) * size) };
}

export function PlayerDisplayMenu({ state, view = unavailableView, ui = DEFAULT_WRIST_MENU_UI }: {
  state: SurfaceState; view?: PlayerDisplayView; ui?: WristMenuUi;
}) {
  const page = ui.page;
  if (page === "home") return null;
  const proximity = view.proximity;
  const viewPriority = view.view_priority;
  const label = (key: string, fallback: string) => view.labels[key] || fallback;
  const named = (key: string, fallback: string, name: string) => label(key, fallback).replaceAll("{{name}}", name);
  const locked = !view.available;
  const current = view.players.filter(player => player.current);
  const history = view.players.filter(player => !player.current);
  const players = pageSlice(ui.group === "current" ? current : history, ui.player_page, 5);
  const languages = pageSlice(view.languages, ui.language_page, 12);
  const hiddenLanguageCount = view.languages.filter(language => language.hidden).length;
  const activePage = page === "players" ? players : languages;
  const paging = (index: number): WristMenuCommand => page === "players" ? { player_page: index } : { language_page: index };
  const message = view.error || (!view.available ? label("unavailable", "Player settings are unavailable") : "");
  const ruleSummary = [
    proximity && `${label("proximityRadius", "Recognition distance")} ${proximity.enabled ? `${proximity.radius_m.toFixed(1)} m` : label("proximityOff", "Off")}`,
    viewPriority && `${label("viewAngle", "Horizontal view")} ${viewPriority.enabled ? `${viewPriority.horizontal_fov_deg}°` : label("proximityOff", "Off")}`,
  ].filter(Boolean).join(" · ");
  const sourceModeButtons = <div className="vr-menu-segments vr-menu-source-segments" role="group" aria-label={label("sourceMode", "Player sources")}>
    {(["everyone", "allowlist"] as const).map((mode, index) => <MenuButton key={mode} state={state} id={48 + index}
      command={{ display: { set_source_mode: mode } }} active={view.source_mode === mode} disabled={locked}
      label={label(mode === "everyone" ? "sourceModeEveryone" : "sourceModeAllowlist", mode === "everyone" ? "Everyone" : "Allowlist")}>
      {label(mode === "everyone" ? "sourceModeEveryone" : "sourceModeAllowlist", mode === "everyone" ? "Everyone" : "Allowlist")}
    </MenuButton>)}
  </div>;
  const pagination = <div className="vr-menu-pagination">
    <MenuButton state={state} id={15} command={paging(Math.max(0, activePage.page - 1))} label={label("previous", "Previous page")} disabled={activePage.page === 0}><ChevronLeft /></MenuButton>
    <span>{activePage.page + 1} / {activePage.count}</span>
    <MenuButton state={state} id={16} command={paging(Math.min(activePage.count - 1, activePage.page + 1))} label={label("next", "Next page")} disabled={activePage.page + 1 >= activePage.count}><ChevronRight /></MenuButton>
  </div>;

  return <section className="vr-player-display-panel vr-surface-panel" data-vr-panel>
    <header className="vr-menu-heading">
      <div><h1>{label("title", "Capture management")}</h1>
        {message && <p className={`vr-menu-status${view.error ? " vr-surface-error" : ""}`} role={view.error ? "alert" : "status"}>{message}</p>}
      </div>
      <MenuButton state={state} id={5} command={{ page: "home" }} label={label("close", "Close")}><X /></MenuButton>
    </header>
    <nav className="vr-menu-tabs">
      <MenuButton state={state} id={6} command={{ page: "players" }} active={page === "players"}><UsersRound /><span>{label("playersTab", "People")}</span></MenuButton>
      <MenuButton state={state} id={52} command={{ page: "rules" }} active={page === "rules"}><SlidersHorizontal /><span>{label("rulesTab", "Rules")}</span></MenuButton>
      <MenuButton state={state} id={7} command={{ page: "languages" }} active={page === "languages"}><Languages /><span>{label("languagesTab", "Languages")}</span>{hiddenLanguageCount > 0 && <small className="vr-menu-count">{hiddenLanguageCount}</small>}</MenuButton>
    </nav>
    {page === "players" ? <>
      <div className="vr-menu-player-toolbar">
        <div className="vr-menu-segments vr-menu-group-segments" role="group" aria-label={label("groups", "People list")}>
          <MenuButton state={state} id={12} command={{ group: "current" }} active={ui.group === "current"}><span>{label("current", "Current world")}</span><small className="vr-menu-count">{current.length}</small></MenuButton>
          <MenuButton state={state} id={13} command={{ group: "history" }} active={ui.group === "history"}><span>{label("historySaved", "History")}</span><small className="vr-menu-count">{history.length}</small></MenuButton>
        </div>
        {sourceModeButtons}
      </div>
      {players.items.length > 0 && <div className="vr-menu-list-columns" aria-hidden="true">
        <span /><span>{label("chatboxLabel", "Chatbox")}</span><span>{label("voiceLabel", "Voice")}</span>
      </div>}
      <div className="vr-menu-player-list">
        {players.items.length ? players.items.map((player, index) => <div className={`vr-menu-player-row vr-menu-track-row${player.hidden ? " is-hidden" : ""}`} key={`${player.track_id ?? "saved"}:${player.user_id ?? "unknown"}`}>
          <div className="vr-menu-player-name"><strong>{player.name}</strong><small>{!player.user_id ? label("userIdPending", "Waiting for player user ID before selection") : player.voice_locked ? `${player.detail} · ${label("voiceLocked", "Hidden record · voice locked")}` : player.detail}</small></div>
          {([["vrchat_chatbox", player.chatbox_disabled, "Chatbox"], ["speaker", player.voice_disabled, "Voice"]] as const).map(([source, disabled, icon], offset) =>
            <MenuButton key={source} state={state} id={33 + index * 3 + offset}
              command={{ display: { set_user_source_disabled: { user_id: player.user_id ?? "", source, disabled: !disabled } } }}
              disabled={locked || !player.user_id || (source === "speaker" && player.voice_locked)}
              active={!disabled} pressed={disabled}
              className={`vr-menu-row-icon-button${disabled ? " is-muted" : ""}`}
              label={named(disabled ? `enable${icon}` : `disable${icon}`,
                disabled ? `Resume ${icon.toLowerCase()} capture for {{name}}` : `Stop ${icon.toLowerCase()} capture for {{name}}`, player.name)}>
              {source === "vrchat_chatbox"
                ? disabled ? <MessageSquareOff /> : <MessageSquare />
                : disabled ? <VolumeX /> : <Volume2 />}
            </MenuButton>)}
        </div>) : <p className="vr-surface-empty">{ui.group === "current" ? label("currentEmpty", label("empty", "Waiting for tracks or loading history.")) : label("historyEmpty", label("empty", "Waiting for tracks or loading history."))}</p>}
      </div>
      <footer className="vr-menu-footer">
        {ruleSummary && <MenuButton state={state} id={53} command={{ page: "rules" }} className="vr-menu-summary-button" label={label("rulesTab", "Rules")}>
          <SlidersHorizontal /><span>{ruleSummary}</span><ChevronRight />
        </MenuButton>}
        {pagination}
      </footer>
    </> : page === "rules" ? <>
      {view.available && (!view.processing_available || !view.player_mode) && <p className="vr-menu-status" role="status">{label("processingUnavailable", "Distance filtering requires per-player audio and a connected Core")}</p>}
      <div className="vr-menu-rules">
        <section className="vr-menu-card" aria-label={label("sourceMode", "Player sources")}>
          <div className="vr-menu-card-head"><h2>{label("sourceMode", "Player sources")}</h2></div>
          {sourceModeButtons}
        </section>
        {proximity && <section className="vr-menu-card" aria-label={label("proximityRadius", "Recognition distance")}>
          <div className="vr-menu-card-head">
            <h2>{label("proximityRadius", "Recognition distance")}</h2>
            <MenuButton state={state} id={54} command={{ display: { set_proximity_radius: 0 } }}
              disabled={locked || !view.player_mode || !proximity.enabled} active={!proximity.enabled} className="vr-menu-card-action">
              {label("proximityOff", "Off")}
            </MenuButton>
          </div>
          <MenuSlider state={state} id={32} value={proximity.enabled ? proximity.radius_m : 0}
            disabled={locked || !view.player_mode} command={PROXIMITY_SLIDER} scale={["0", "10 m"]}
            label={label("proximityRadius", "Recognition distance")}
            formatValue={radius => radius > 0 ? `${radius.toFixed(1)} m` : label("proximityOff", "Off")} error={view.error} />
        </section>}
        {viewPriority && <section className="vr-menu-card" aria-label={label("viewPriority", "Prioritize players in view")}>
          <div className="vr-menu-card-head">
            <h2><Eye /><span>{label("viewPriority", "Prioritize players in view")}</span></h2>
            <MenuButton state={state} id={50} active={viewPriority.enabled} disabled={locked || !view.player_mode}
              command={{ display: { set_view_priority: { enabled: !viewPriority.enabled, horizontal_fov_deg: viewPriority.horizontal_fov_deg } } }}
              label={label("viewPriority", "Prioritize players in view")} className="vr-menu-switch-button">
              <span className="vr-menu-toggle" aria-hidden="true"><i /></span>
            </MenuButton>
          </div>
          <div className="vr-menu-slider-row">
            <span className="vr-menu-slider-label">{label("viewAngle", "Horizontal view")}</span>
            <MenuSlider state={state} id={51} value={viewPriority.horizontal_fov_deg}
              disabled={locked || !view.player_mode || !viewPriority.enabled} command={VIEW_ANGLE_SLIDER} scale={["30°", "180°"]}
              label={label("viewAngle", "Horizontal view")} formatValue={degrees => `${degrees}°`} error={view.error} />
          </div>
          {viewPriority.enabled && !viewPriority.local_view && <p className="vr-menu-card-note" role="status">{label("viewWaiting", "Waiting for view data; players currently run at normal priority.")}</p>}
        </section>}
      </div>
    </> : <>
      <div className="vr-menu-language-list">
        {languages.items.map((language, index) => <MenuButton state={state} id={17 + index} key={language.code} className="vr-menu-language-option"
          command={{ display: { set_source_language_hidden: { language: language.code, hidden: !language.hidden } } }} active={language.hidden} disabled={locked}>
          <span className="vr-menu-check" aria-hidden="true">{language.hidden && <Check />}</span><span>{language.label}</span>
        </MenuButton>)}
        {!languages.items.length && <p className="vr-surface-empty">{label("noHiddenLanguages", "No languages hidden")}</p>}
      </div>
      <footer className="vr-menu-footer">
        <div className="vr-menu-footer-actions">
          <MenuButton state={state} id={9} command={{ display: { clear_hidden_languages: null } }} disabled={locked || hiddenLanguageCount === 0}>{label("noHiddenLanguages", "No languages hidden")}</MenuButton>
          {hiddenLanguageCount > 0 && <span className="vr-menu-footer-note">{label("hiddenLanguagesCount", "{{count}} hidden").replaceAll("{{count}}", String(hiddenLanguageCount))}</span>}
        </div>
        {pagination}
      </footer>
    </>}
  </section>;
}
