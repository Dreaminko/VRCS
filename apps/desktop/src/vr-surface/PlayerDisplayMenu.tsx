import { useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { Check, ChevronLeft, ChevronRight, Eye, Info, Languages, MessageSquare, MessageSquareOff, Radar, SlidersHorizontal, TriangleAlert, UsersRound, Volume2, VolumeX, X } from "lucide-react";
import type { PlayerDisplayView, SliderCommand, WristMenuCommand, WristMenuUi } from "../vr-controls/player-display-protocol";
import type { SurfaceState } from "./protocol";

export const DEFAULT_WRIST_MENU_UI: WristMenuUi = { page: "home", group: "current", player_page: 0, language_page: 0 };

const DEFAULT_PROXIMITY_RADIUS = 5;
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

function RuleSliderCard({
  state, icon, title, subtitle, value, cardDisabled, sliderDisabled,
  toggleId, toggleCommand, toggleChecked, sliderId, sliderCommand, scale,
  formatValue, valueMuted, zeroStyled, note, error,
}: {
  state: SurfaceState;
  icon: ReactNode;
  title: string;
  subtitle?: string;
  value: number;
  cardDisabled: boolean;
  sliderDisabled: boolean;
  toggleId: number;
  toggleCommand: WristMenuCommand;
  toggleChecked: boolean;
  sliderId: number;
  sliderCommand: SliderCommand;
  scale: readonly [string, string];
  formatValue: (value: number) => string;
  valueMuted: (shown: number) => boolean;
  zeroStyled?: (shown: number) => boolean;
  note?: string;
  error: string | null;
}) {
  const track = useRef<HTMLButtonElement>(null);
  const dragging = useRef(false);
  const errorAtPress = useRef<string | null>(null);
  const valueAtPress = useRef<number>(value);
  const [preview, setPreview] = useState<number | null>(null);
  const pointer = state.pointer;
  const pressed = state.pressed === sliderId;

  useLayoutEffect(() => {
    if (pressed && !dragging.current) {
      errorAtPress.current = error;
      valueAtPress.current = value;
    } else if (error === null) {
      errorAtPress.current = null;
    }
    dragging.current = pressed;
    const element = track.current;
    const root = element?.closest<HTMLElement>(".vr-surface");
    if (sliderDisabled || !pressed || !pointer || !element || !root) return;
    const surface = root.getBoundingClientRect();
    const bounds = element.getBoundingClientRect();
    if (!surface.width || !bounds.width) return;
    const x = pointer[0] * surface.width - (bounds.left - surface.left);
    const fraction = Math.max(0, Math.min(1, x / bounds.width));
    const raw = sliderCommand.min + fraction * (sliderCommand.max - sliderCommand.min);
    const quantized = sliderCommand.min + Math.round((raw - sliderCommand.min) / sliderCommand.step) * sliderCommand.step;
    setPreview(Math.round(Math.max(sliderCommand.min, Math.min(sliderCommand.max, quantized)) * 10) / 10);
  }, [error, pointer, pressed, sliderCommand, sliderDisabled, value]);

  useLayoutEffect(() => {
    if (sliderDisabled || (preview !== null && (
      value === preview
      || (!pressed && value !== valueAtPress.current)
      || (error !== null && error !== errorAtPress.current)
    ))) setPreview(null);
  }, [error, pressed, preview, sliderDisabled, value]);

  const shown = preview ?? Math.max(sliderCommand.min, Math.min(sliderCommand.max, value));
  const text = formatValue(shown);
  const progress = (shown - sliderCommand.min) / (sliderCommand.max - sliderCommand.min) * 100;
  const sliderHovered = !sliderDisabled && state.hovered === sliderId;
  const sliderPressed = !sliderDisabled && pressed;

  return <section className={`vr-menu-card${cardDisabled ? " is-disabled" : ""}`} aria-label={title}>
    <div className="vr-menu-card-head">
      <div className="vr-menu-card-title">
        {icon}
        <div>
          <h2>{title}</h2>
          {subtitle && <small>{subtitle}</small>}
        </div>
      </div>
      <div className="vr-menu-card-controls">
        <strong className={`vr-menu-value-pill${valueMuted(shown) ? " is-muted" : ""}`}>{text}</strong>
        <MenuButton state={state} id={toggleId} active={toggleChecked} disabled={cardDisabled}
          command={toggleCommand} label={title} className="vr-menu-switch-button">
          <span className="vr-menu-toggle" aria-hidden="true"><i /></span>
        </MenuButton>
      </div>
    </div>
    <div className="vr-menu-slider">
      <button type="button" tabIndex={-1} role="slider" ref={track} disabled={sliderDisabled}
        data-vr-region={sliderId} data-vr-menu={JSON.stringify({ slider: sliderCommand })}
        aria-label={subtitle ?? title} aria-orientation="horizontal"
        aria-valuemin={sliderCommand.min} aria-valuemax={sliderCommand.max} aria-valuenow={shown}
        aria-valuetext={text}
        className={`vr-menu-slider-track${sliderHovered ? " hovered" : ""}${sliderPressed ? " pressed" : ""}${zeroStyled?.(shown) ? " is-zero" : ""}`}
        style={{ "--vr-slider-progress": `${progress}%` } as CSSProperties}>
        <span className="vr-menu-slider-line" aria-hidden="true">
          <span className="vr-menu-slider-rail"><span className="vr-menu-slider-fill" /></span>
          <span className="vr-menu-slider-thumb" />
          <span className="vr-menu-slider-scale"><span>{scale[0]}</span><span>{scale[1]}</span></span>
        </span>
      </button>
    </div>
    {note && <p className="vr-menu-card-note" role="status"><Info aria-hidden="true" /><span>{note}</span></p>}
  </section>;
}

function pageSlice<T>(items: readonly T[], requested: number, size: number) {
  const count = Math.max(1, Math.ceil(items.length / size));
  const page = Math.max(0, Math.min(count - 1, Number.isFinite(requested) ? Math.floor(requested) : 0));
  return { page, count, items: items.slice(page * size, (page + 1) * size) };
}

export function PlayerDisplayMenu({ state, view = unavailableView, ui = DEFAULT_WRIST_MENU_UI }: {
  state: SurfaceState; view?: PlayerDisplayView; ui?: WristMenuUi;
}) {
  const lastProximityRadius = useRef<number | null>(null);
  const proximity = view.proximity;
  if (proximity?.enabled && proximity.radius_m > 0) {
    lastProximityRadius.current = Math.round(Math.max(0.1, Math.min(10, proximity.radius_m)) * 10) / 10;
  }
  const page = ui.page;
  if (page === "home") return null;
  const viewPriority = view.view_priority;
  const proximityAvailable = Boolean(view.proximity_available || view.player_mode);
  const restoreProximityRadius = proximity && proximity.radius_m > 0
    ? Math.round(Math.max(0.1, Math.min(10, proximity.radius_m)) * 10) / 10
    : (lastProximityRadius.current ?? DEFAULT_PROXIMITY_RADIUS);
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
    viewPriority && `${label("viewAngle", "Horizontal view")} ${viewPriority.enabled ? `${Math.round(viewPriority.horizontal_fov_deg)}°` : label("proximityOff", "Off")}`,
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
      <div className="vr-menu-rules">
        <section className={`vr-menu-card vr-menu-card-inline${locked ? " is-disabled" : ""}`} aria-label={label("sourceMode", "Player sources")}>
          <div className="vr-menu-card-title">
            <UsersRound aria-hidden="true" />
            <div><h2>{label("sourceMode", "Player sources")}</h2></div>
          </div>
          {sourceModeButtons}
        </section>
        {view.available && (!view.processing_available || !view.player_mode) && <div className="vr-menu-banner warning" role="status">
          <TriangleAlert aria-hidden="true" />
          <span>{label("processingUnavailable", "Distance filtering requires per-player audio or Chatbox capture; view priority requires per-player audio and a connected Core.")}</span>
        </div>}
        {proximity && <RuleSliderCard
          state={state}
          icon={<Radar aria-hidden="true" />}
          title={label("proximityRadius", "Recognition distance")}
          value={proximity.enabled ? proximity.radius_m : 0}
          cardDisabled={locked || !proximityAvailable}
          sliderDisabled={locked || !proximityAvailable}
          toggleId={54}
          toggleChecked={proximity.enabled}
          toggleCommand={{ display: { set_proximity_radius: proximity.enabled ? 0 : restoreProximityRadius } }}
          sliderId={32}
          sliderCommand={PROXIMITY_SLIDER}
          scale={[label("proximityOff", "Off"), "10 m"]}
          formatValue={radius => radius > 0 ? `${radius.toFixed(1)} m` : label("proximityOff", "Off")}
          valueMuted={shown => locked || !proximityAvailable || shown <= 0}
          zeroStyled={shown => shown <= 0}
          error={view.error}
        />}
        {viewPriority && <RuleSliderCard
          state={state}
          icon={<Eye aria-hidden="true" />}
          title={label("viewPriority", "Prioritize players in view")}
          subtitle={label("viewAngle", "Horizontal view")}
          value={viewPriority.horizontal_fov_deg}
          cardDisabled={locked || !view.player_mode}
          sliderDisabled={locked || !view.player_mode || !viewPriority.enabled}
          toggleId={50}
          toggleChecked={viewPriority.enabled}
          toggleCommand={{ display: { set_view_priority: { enabled: !viewPriority.enabled, horizontal_fov_deg: viewPriority.horizontal_fov_deg } } }}
          sliderId={51}
          sliderCommand={VIEW_ANGLE_SLIDER}
          scale={["30°", "180°"]}
          formatValue={degrees => `${Math.round(degrees)}°`}
          valueMuted={() => locked || !view.player_mode || !viewPriority.enabled}
          note={view.available && view.player_mode && view.processing_available && viewPriority.enabled && !viewPriority.local_view
            ? label("viewWaiting", "Waiting for view data; players currently run at normal priority.")
            : undefined}
          error={view.error}
        />}
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
