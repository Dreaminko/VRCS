import type { ReactNode } from "react";
import { Check, ChevronLeft, ChevronRight, Eye, EyeOff, Languages, Mic, UsersRound, X } from "lucide-react";
import type { PlayerDisplayView, WristMenuCommand, WristMenuUi } from "../vr-controls/player-display-protocol";
import type { SurfaceState } from "./protocol";

export const DEFAULT_WRIST_MENU_UI: WristMenuUi = { page: "home", group: "current", player_page: 0, language_page: 0 };

const unavailableView: PlayerDisplayView = {
  available: false, processing_available: false, player_mode: false, selection_mode: "all", selected_count: 0,
  players: [], languages: [], follow_default: false, default_languages: [], labels: {}, busy: false, error: null,
};

function MenuButton({ state, id, command, children, label, active = false, disabled = false, className = "" }: {
  state: SurfaceState; id: number; command: WristMenuCommand; children: ReactNode; label?: string;
  active?: boolean; disabled?: boolean; className?: string;
}) {
  return <button type="button" tabIndex={-1} data-vr-region={id} data-vr-menu={JSON.stringify(command)}
    disabled={disabled} aria-label={label} aria-pressed={active}
    className={`vr-menu-button${active ? " active" : ""}${state.hovered === id ? " hovered" : ""}${state.pressed === id ? " pressed" : ""}${className ? ` ${className}` : ""}`}>
    {children}
  </button>;
}

function pageSlice<T>(items: readonly T[], requested: number, size: number) {
  const count = Math.max(1, Math.ceil(items.length / size));
  const page = Math.max(0, Math.min(count - 1, Number.isFinite(requested) ? Math.floor(requested) : 0));
  return { page, count, items: items.slice(page * size, (page + 1) * size) };
}

export function PlayerDisplayMenu({ state, view = unavailableView, ui = DEFAULT_WRIST_MENU_UI }: {
  state: SurfaceState; view?: PlayerDisplayView; ui?: WristMenuUi;
}) {
  const page = ui.page === "microphone" && !view.microphone ? "players" : ui.page;
  if (page === "home") return null;
  const label = (key: string, fallback: string) => view.labels[key] || fallback;
  const named = (key: string, fallback: string, name: string) => label(key, fallback).replaceAll("{{name}}", name);
  const locked = view.busy || !view.available;
  const processingLocked = locked || !view.processing_available || !view.player_mode;
  const selectionLocked = processingLocked || view.selection_mode !== "selected";
  const current = view.players.filter(player => player.current);
  const players = pageSlice(ui.group === "current" ? current : view.players.filter(player => !player.current), ui.player_page, 5);
  const languages = pageSlice(view.languages, ui.language_page, 12);
  const mic = view.microphone;
  const microphones = pageSlice(mic?.speakers ?? [], ui.microphone_page ?? 0, 5);
  const micReady = !!mic?.supported && mic.running && !!mic.generation;
  const micStatus = !mic ? label("mic_unavailable", "Waiting for microphone status")
    : !mic.running ? null
    : !mic.supported ? label("mic_unsupported", "Requires Qwen Live Translate with speaker detection")
    : !mic.generation ? label("mic_connecting", "Waiting for recognition connection")
    : label("mic_sessionOnly", "Blocks apply to this connection; block speakers again after reconnecting");
  const activePage = page === "players" ? players : page === "microphone" ? microphones : languages;
  const paging = (index: number): WristMenuCommand => page === "players" ? { player_page: index } : page === "microphone" ? { microphone_page: index } : { language_page: index };
  const message = view.error || (view.busy ? label("busy", "Working") : !view.available ? label("unavailable", "Player settings are unavailable")
    : page === "players" && (!view.processing_available || !view.player_mode) ? label("processingUnavailable", "Player processing is unavailable; display controls still work")
      : page === "players" && view.selection_mode === "selected" && !current.some(player => player.selected)
        ? label("noneSelected", "No current players selected; other-party audio is not being processed.") : "");
  const pagination = <div className="vr-menu-pagination">
    <MenuButton state={state} id={15} command={paging(Math.max(0, activePage.page - 1))} label={label("previous", "Previous page")} disabled={activePage.page === 0}><ChevronLeft /></MenuButton>
    <span>{activePage.page + 1} / {activePage.count}</span>
    <MenuButton state={state} id={16} command={paging(Math.min(activePage.count - 1, activePage.page + 1))} label={label("next", "Next page")} disabled={activePage.page + 1 >= activePage.count}><ChevronRight /></MenuButton>
  </div>;

  return <section className="vr-player-display-panel vr-surface-panel" data-vr-panel aria-busy={view.busy || undefined}>
    <header className="vr-menu-heading">
      <div><h1>{label("title", "People and translation display")}</h1>
        {message && <p className={`vr-menu-status${view.error ? " vr-surface-error" : ""}`} role={view.error ? "alert" : "status"}>{message}</p>}
      </div>
      <MenuButton state={state} id={5} command={{ page: "home" }} label={label("close", "Close")}><X /></MenuButton>
    </header>
    <nav className="vr-menu-tabs">
      <MenuButton state={state} id={6} command={{ page: "players" }} active={page === "players"}><UsersRound /><span>{label("playersTab", "People")}</span></MenuButton>
      <MenuButton state={state} id={7} command={{ page: "languages" }} active={page === "languages"}><Languages /><span>{label("languages", "Hidden original languages")}</span></MenuButton>
      {mic && <MenuButton state={state} id={29} command={{ page: "microphone" }} active={page === "microphone"}><Mic /><span>{label("mic_title", "Microphone speakers")}</span></MenuButton>}
    </nav>
    {page === "players" ? <>
      <div className="vr-menu-processing" aria-label={label("process", "Players to process")} data-available={!processingLocked}>
        <MenuButton state={state} id={8} command={{ display: { set_processing_mode: "all" } }} active={view.selection_mode === "all"} disabled={processingLocked || view.selection_mode === "all"}>{label("all", "Translate everyone")}</MenuButton>
        <MenuButton state={state} id={9} command={{ display: { set_processing_mode: "selected" } }} active={view.selection_mode === "selected"} disabled={processingLocked || view.selection_mode === "selected"}>{label("selected", "Translate selected people only")}</MenuButton>
      </div>
      <div className="vr-menu-player-actions">
        <MenuButton state={state} id={10} command={{ display: { select_current: null } }} disabled={selectionLocked || !current.some(player => player.user_id)}>{label("selectCurrent", "Select everyone here")}</MenuButton>
        <MenuButton state={state} id={11} command={{ display: { clear_selection: null } }} disabled={selectionLocked || !view.selected_count}>{label("selectNone", "Deselect all")}</MenuButton>
        <MenuButton state={state} id={14} command={{ display: { show_all_tracks: null } }} disabled={locked}>{label("showAll", "Show all records")}</MenuButton>
      </div>
      <nav className="vr-menu-player-groups">
        <MenuButton state={state} id={12} command={{ group: "current" }} active={ui.group === "current"}>{label("current", "Current world")}</MenuButton>
        <MenuButton state={state} id={13} command={{ group: "history" }} active={ui.group === "history"}>{label("historySaved", "History / saved")}</MenuButton>
        {pagination}
      </nav>
      <div className="vr-menu-player-list">
        {players.items.length ? players.items.map((player, index) => <div className={`vr-menu-player-row${player.hidden ? " is-hidden" : ""}`} key={`${player.track_id ?? "saved"}:${player.user_id ?? "unknown"}`}>
          <div className="vr-menu-player-name"><strong>{player.name}</strong><small>{!player.user_id ? label("userIdPending", "Waiting for player user ID before selection") : player.detail}</small></div>
          <MenuButton state={state} id={17 + index * 2} command={{ display: { set_user_selected: { user_id: player.user_id ?? "", selected: !player.selected } } }}
            disabled={selectionLocked || !player.user_id} active={player.selected} label={named("translatePerson", "Translate {{name}}", player.name)}>
            <span className="vr-menu-check" aria-hidden="true">{player.selected && <Check />}</span><span>{label("translate", "Translate")}</span>
          </MenuButton>
          <MenuButton state={state} id={18 + index * 2} command={{ display: { set_track_hidden: { track_id: player.track_id ?? "", hidden: !player.hidden } } }}
            disabled={locked || !player.track_id} active={player.hidden} label={named(player.hidden ? "showPerson" : "hidePerson", player.hidden ? "Show records from {{name}}" : "Hide records from {{name}}", player.name)}>
            {player.hidden ? <EyeOff /> : <Eye />}<span>{label(player.hidden ? "show" : "hide", player.hidden ? "Show" : "Hide")}</span>
          </MenuButton>
        </div>) : <p className="vr-surface-empty">{label("empty", "Waiting for tracks or loading history.")}</p>}
      </div>
    </> : page === "microphone" ? <>
      <div className="vr-menu-processing">
        <MenuButton state={state} id={8} command={{ display: { set_microphone_filter: !mic?.enabled } }} active={mic?.enabled}
          disabled={locked || !mic}>
          <span className="vr-menu-check" aria-hidden="true">{mic?.enabled && <Check />}</span>{label("mic_enable", "Filter microphone speakers")}
        </MenuButton>
        {pagination}
      </div>
      {micStatus && <p className="vr-menu-status" role="status">{micStatus}</p>}
      <div className="vr-menu-player-list">
        {microphones.items.map((speaker, index) => <div className="vr-menu-player-row" key={speaker.id}>
          <div className="vr-menu-player-name"><strong>{speaker.name}</strong><small>{label(`mic_${speaker.decision}`, speaker.decision)} · {speaker.last_active}</small></div>
          {(["allow", "block"] as const).map((decision, offset) => <MenuButton state={state} id={17 + index * 2 + offset} key={decision}
            command={{ display: { set_microphone_speaker: { generation: mic!.generation, revision: mic!.revision, id: speaker.id, decision } } }}
            active={speaker.decision === decision} disabled={locked || !micReady || !mic?.enabled || speaker.decision === decision}>
            {label(decision === "allow" ? "mic_allowAction" : "mic_blockAction", decision === "allow" ? "Allow" : "Block")}
          </MenuButton>)}
        </div>)}
        {micReady && !microphones.items.length && <p className="vr-surface-empty">{label("mic_empty", "Speak to discover speakers")}</p>}
      </div>
    </> : <>
      <div className="vr-menu-language-actions">
        <MenuButton state={state} id={9} command={{ display: { clear_hidden_languages: null } }} disabled={locked}>{label("noHiddenLanguages", "No languages hidden")}</MenuButton>
      </div>
      <div className="vr-menu-language-list">
        {languages.items.map((language, index) => <MenuButton state={state} id={17 + index} key={language.code} className="vr-menu-language-option"
          command={{ display: { set_source_language_hidden: { language: language.code, hidden: !language.hidden } } }} active={language.hidden} disabled={locked}>
          <span className="vr-menu-check" aria-hidden="true">{language.hidden && <Check />}</span><span>{language.label}</span>
        </MenuButton>)}
        {!languages.items.length && <p className="vr-surface-empty">{label("noHiddenLanguages", "No languages hidden")}</p>}
      </div>
      <footer className="vr-menu-language-footer">{pagination}</footer>
    </>}
  </section>;
}
