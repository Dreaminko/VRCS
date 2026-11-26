import type { ReactNode } from "react";
import { ArrowDown, ArrowUp, Bookmark, Check, ChevronLeft, ChevronRight, Eye, Hand, Languages, Mic, Minus, Monitor, Pin, Plus, Power, Radar, Radio, RotateCcw, ScanEye, Settings2, SlidersHorizontal, Square, Trash2, TriangleAlert, Volume2, X } from "lucide-react";
import type { VrDashboardAppearanceField, VrDashboardChoice, VrDashboardViewModel } from "../integrations/vr-overlay-native";
import type { VrOverlayAppearanceField, VrOverlayPositionField } from "../settings/vr-overlay-settings";
import dashboardThumbnail from "../../src-tauri/icons/128x128@2x.png";
import { DASHBOARD_PICKER_PAGE_SIZE, dashboardPickerChoice, type DashboardAction, type DashboardCommand, type DashboardPicker, type DashboardUi, type DisplayKind, type TargetGroup } from "./dashboard-protocol";
import "./dashboard.css";
import { PlayerAudioPrompt } from "./PlayerAudioPrompt";

type ButtonOptions = {
  active?: boolean;
  disabled?: boolean;
  icon?: boolean;
  primary?: boolean;
  danger?: boolean;
  recording?: boolean;
  className?: string;
  label?: string;
  key?: string;
};

export function DashboardThumbnail() {
  return <img className="vr-dashboard-thumbnail" src={dashboardThumbnail} alt="VRCS" />;
}

export function DashboardSurface({ dashboard: view, ui, hovered, pressed }: {
  dashboard: VrDashboardViewModel; ui: DashboardUi; hovered: number | null; pressed: number | null;
}) {
  if (view.player_audio_notice) return <PlayerAudioPrompt notice={view.player_audio_notice} hovered={hovered} pressed={pressed} dashboard />;
  const labels = view.labels;
  let region = 0;
  const button = (command: DashboardCommand, children: ReactNode, options: ButtonOptions = {}) => {
    const id = region++;
    return <button key={options.key} type="button" tabIndex={-1} data-vr-region={id} data-vr-command={JSON.stringify(command)} disabled={options.disabled}
      aria-label={options.label} aria-pressed={options.active}
      className={`vr-dashboard-button${options.icon ? " icon" : ""}${options.active ? " active" : ""}${options.primary ? " primary" : ""}${options.danger ? " danger" : ""}${options.recording ? " recording" : ""}${hovered === id ? " hovered" : ""}${pressed === id ? " pressed" : ""}${options.className ? ` ${options.className}` : ""}`}>
      {children}
    </button>;
  };
  const action = (value: DashboardAction, children: ReactNode, options: ButtonOptions = {}) => button({ type: "action", value }, children, options);
  const toggle = (value: DashboardAction, label: string, enabled: boolean, disabled = false, sublabel?: string) => action(value, <>
    {label && <span className="vr-dashboard-btn-copy"><span>{label}</span>{sublabel && <small className="vr-dashboard-btn-sub">{sublabel}</small>}</span>}
    <span className="vr-dashboard-toggle" aria-hidden="true"><i /></span>
  </>, { active: enabled, disabled, className: label ? "vr-dashboard-toggle-row" : "vr-dashboard-switch-only" });
  const segments = (choice: VrDashboardChoice, toAction: (value: string) => DashboardAction, disabled = false) => <div className="vr-dashboard-segments">
    {choice.options.map((option) => action(toAction(option.value), <span>{option.label}</span>, { active: choice.value === option.value || choice.value === option.label, disabled, key: option.value }))}
  </div>;
  const stepper = (
    title: string,
    value: string,
    down: DashboardAction,
    up: DashboardAction,
    canDecrease = true,
    canIncrease = true,
    key?: string,
  ) => <div className="vr-dashboard-stepper" key={key}>
    <span>{title}</span>
    <div>
      {action(down, <Minus />, { icon: true, disabled: !canDecrease, label: `${title} −` })}
      <strong>{value}</strong>
      {action(up, <Plus />, { icon: true, disabled: !canIncrease, label: `${title} +` })}
    </div>
  </div>;
  const pickerButton = (picker: DashboardPicker, label: string, choice: VrDashboardChoice, note?: string, icon?: ReactNode, className?: string) => button({ type: "picker", value: picker }, <>
    {icon}
    <span className="vr-dashboard-picker-label"><span>{label}</span>{note && <small>{note}</small>}</span>
    <strong>{choice.value}</strong>
    <ChevronRight />
  </>, { disabled: !choice.options.length, className });

  const appearanceMap = (fields?: VrDashboardAppearanceField[]) => new Map(fields?.map((item) => [item.field, item]));

  const displayCard = (kind: DisplayKind) => {
    const display = view[kind];
    const wrist = kind === "wrist";
    const fields = appearanceMap(display.appearance);
    const widthField = fields.get("width_m");
    const opacityField = fields.get("opacity");
    const fontField = fields.get("font_size_px");
    const stepAppearance = (field: VrOverlayAppearanceField, item: VrDashboardAppearanceField | undefined, title: string, value: string, legacyDown: DashboardAction, legacyUp: DashboardAction) => item
      ? stepper(title, item.value, { adjust_display_appearance: { kind, field, direction: -1, coarse: false } }, { adjust_display_appearance: { kind, field, direction: 1, coarse: false } }, item.can_decrease, item.can_increase, field)
      : stepper(title, value, legacyDown, legacyUp, true, true, field);
    return <section className={`vr-dashboard-card${!view.enabled || !display.enabled ? " is-muted" : ""}`} key={kind}>
      <header>
        {wrist ? <Hand /> : <Monitor />}
        <h2>{wrist ? labels.wrist : labels.headset}</h2>
        {wrist && view.wrist.anchor_mode === "world" && <span className="vr-dashboard-pill">{labels.anchor_world || "World"}</span>}
        {toggle(wrist ? "toggle_wrist" : "toggle_headset", "", display.enabled)}
      </header>
      <div className="vr-dashboard-card-options">
        {wrist
          ? <><span>{labels.hand}</span>{segments(view.wrist.hand, (value) => ({ set_wrist_hand: value }))}</>
          : <p>{labels.headset_hint}</p>}
      </div>
      <div className="vr-dashboard-steppers">
        {stepAppearance("width_m", widthField, labels.width, display.width, wrist ? "wrist_width_down" : "headset_width_down", wrist ? "wrist_width_up" : "headset_width_up")}
        {stepAppearance("opacity", opacityField, labels.opacity, display.opacity, wrist ? "wrist_opacity_down" : "headset_opacity_down", wrist ? "wrist_opacity_up" : "headset_opacity_up")}
        {fontField && stepper(fontField.label, fontField.value, { adjust_display_appearance: { kind, field: "font_size_px", direction: -1, coarse: false } }, { adjust_display_appearance: { kind, field: "font_size_px", direction: 1, coarse: false } }, fontField.can_decrease, fontField.can_increase, "font_size_px")}
      </div>
      <div className="vr-dashboard-card-actions">
        {button({ type: "position", value: kind }, <><Settings2 /><span>{labels.position}</span></>)}
        {action(wrist ? "preview_wrist" : "preview_headset", <><Eye /><span>{display.preview_visible ? labels.hide_preview : labels.preview}</span></>, { active: display.preview_visible, disabled: !view.enabled || !display.enabled })}
      </div>
    </section>;
  };

  const languageTargets = (group: TargetGroup) => {
    const targets = view.language[`${group}_targets`];
    return <section className="vr-dashboard-card" key={group}>
      <header>{group === "microphone" ? <Mic /> : <Volume2 />}<h2>{group === "microphone" ? labels.microphone_language : labels.speaker_language}</h2></header>
      <div className="vr-dashboard-targets">{targets.map((choice, index) => <div className="vr-dashboard-target" key={index}>
        {button({ type: "picker", value: { type: "target", value: { group, index } } }, <>
          {index === 0 ? <span className="vr-dashboard-target-badge">{labels.preferred || "Primary"}</span> : <span className="vr-dashboard-target-index">{index + 1}</span>}
          <strong>{choice.value}</strong>
          <ChevronRight />
        </>, { disabled: !choice.options.length })}
        {targets.length > 1 && <>
          {action({ move_target: { group, index, offset: -1 } }, <ArrowUp />, { icon: true, disabled: index === 0, label: "↑" })}
          {action({ move_target: { group, index, offset: 1 } }, <ArrowDown />, { icon: true, disabled: index + 1 >= targets.length, label: "↓" })}
          {action({ delete_target: { group, index } }, <Trash2 />, { icon: true, danger: true, label: labels.delete })}
        </>}
      </div>)}</div>
      {action({ add_target: group }, <><Plus /><span>{labels.add_target}</span></>, { disabled: !view.language.can_add || targets.length >= 3 })}
    </section>;
  };

  const positionEditor = (kind: DisplayKind) => {
    const display = view[kind];
    const fieldGroup = (rotation: boolean) => <div className="vr-dashboard-position-grid">{display.position.filter((item) => ["pitch_deg", "yaw_deg", "roll_deg"].includes(item.field) === rotation).map((item) => {
      const adjust = (direction: -1 | 1) => ({ adjust_display_position: { kind, field: item.field as VrOverlayPositionField, direction, coarse: false } });
      return stepper(item.label, item.value, adjust(-1), adjust(1), item.can_decrease, item.can_increase, item.field);
    })}</div>;
    const wristWorld = kind === "wrist" && view.wrist.anchor_mode === "world";
    return <section className="vr-dashboard-card vr-dashboard-position">
      <header>
        {button({ type: "close" }, <><ChevronLeft /><span>{labels.back || labels.close}</span></>, { className: "vr-dashboard-back" })}
        <div className="vr-dashboard-subpage-title">{kind === "wrist" ? <Hand /> : <Monitor />}<h2>{kind === "wrist" ? labels.wrist : labels.headset} · {labels.position}</h2></div>
        {action(kind === "wrist" ? "preview_wrist" : "preview_headset", <><Eye /><span>{display.preview_visible ? labels.hide_preview : labels.preview}</span></>, { active: display.preview_visible, disabled: !view.enabled || !display.enabled })}
      </header>
      <div className="vr-dashboard-position-section">
        <h3><span>{labels.position}</span><small>{labels.position_hint}</small></h3>
        {fieldGroup(false)}
      </div>
      <div className="vr-dashboard-position-section">
        <h3><span>{labels.rotation}</span><small>{labels.rotation_hint}</small></h3>
        {fieldGroup(true)}
      </div>
      {kind === "wrist" && <div className="vr-dashboard-anchor-bar">
        <span>{labels.anchor_mode || labels.position}</span>
        <div className="vr-dashboard-segments">
          {action("attach_dialogue", <span>{labels.attach_dialogue}</span>, { active: !wristWorld })}
          {action("recenter_dialogue", <span>{labels.anchor_world || "World"}</span>, { active: wristWorld })}
        </div>
        {toggle("toggle_wrist_follow_view", labels.follow_view || "Follow view", view.wrist.follow_view, !wristWorld)}
        {action("recenter_dialogue", <><Pin /><span>{labels.recenter_dialogue}</span></>, { disabled: !wristWorld })}
      </div>}
      <div className="vr-dashboard-position-actions">
        {action({ reset_display_position: kind }, <><RotateCcw /><span>{labels.reset_position}</span></>)}
      </div>
    </section>;
  };

  const picker = ui.picker;
  const choice = picker ? dashboardPickerChoice(view, picker) : undefined;
  const pickerTitle = picker?.type === "recognition" ? labels.recognition_language
    : picker?.type === "mode" ? labels.translation_mode
    : picker?.type === "target" ? `${picker.value.group === "microphone" ? labels.microphone_language : labels.speaker_language} · #${picker.value.index + 1}`
    : picker?.type === "delete_preset" ? (labels.delete_preset_title || labels.delete)
    : labels.apply_preset;
  const pickerIcon = picker?.type === "delete_preset" ? <Trash2 className="danger-icon" />
    : picker?.type === "apply_preset" ? <Bookmark />
    : picker?.type === "mode" ? <SlidersHorizontal />
    : <Languages />;
  const totalPages = choice ? Math.max(1, Math.ceil(choice.options.length / DASHBOARD_PICKER_PAGE_SIZE)) : 1;
  const compactPicker = Boolean(choice && choice.options.length <= 6);
  const translationDisabled = view.language.mode.options.some((option) => option.value === "disabled" && (view.language.mode.value === option.label || view.language.mode.value === option.value));

  const confirmationTitle = ui.confirmation
    ? typeof ui.confirmation.action === "object" && "delete_language_preset" in ui.confirmation.action
      ? (labels.delete_preset_title || labels.confirm_delete)
      : labels.confirm_delete
    : "";

  const content = ui.confirmation ? <section className="vr-dashboard-card vr-dashboard-confirmation">
    <div className="vr-dashboard-confirm-icon"><Trash2 /></div>
    <h2>{confirmationTitle}</h2>
    <strong>{ui.confirmation.label}</strong>
    <p>{labels.delete_hint}</p>
    <div className="vr-dashboard-confirm-actions">{button({ type: "cancel" }, labels.cancel)}{button({ type: "confirm" }, labels.delete, { danger: true })}</div>
  </section> : choice ? <section className="vr-dashboard-card vr-dashboard-picker">
    <header>
      {button({ type: "close" }, <><ChevronLeft /><span>{labels.back || labels.close}</span></>, { className: "vr-dashboard-back" })}
      <div className="vr-dashboard-subpage-title">{pickerIcon}<h2>{pickerTitle}</h2></div>
      {totalPages > 1 && <div className="vr-dashboard-picker-pager">
        <span>{ui.picker_page + 1} / {totalPages}</span>
        {button({ type: "picker_page", value: -1 }, <ChevronLeft />, { icon: true, disabled: ui.picker_page === 0, label: "←" })}
        {button({ type: "picker_page", value: 1 }, <ChevronRight />, { icon: true, disabled: ui.picker_page + 1 >= totalPages, label: "→" })}
      </div>}
    </header>
    <div className={`vr-dashboard-picker-options${compactPicker ? (choice.options.length <= 3 ? " compact single" : " compact") : ""}`}>
      {choice.options.slice(ui.picker_page * DASHBOARD_PICKER_PAGE_SIZE, (ui.picker_page + 1) * DASHBOARD_PICKER_PAGE_SIZE).map((option, index) => {
        const deleting = picker?.type === "delete_preset";
        const selected = !deleting && (choice.value === option.value || choice.value === option.label);
        return button({ type: "select", value: ui.picker_page * DASHBOARD_PICKER_PAGE_SIZE + index }, <>
          <span>{option.label}</span>
          {selected && <Check />}
        </>, { active: selected, danger: deleting, key: option.value });
      })}
    </div>
  </section> : ui.display_editor ? positionEditor(ui.display_editor) : ui.page === "display" ? <>
    <section className="vr-dashboard-content-choice"><span>{labels.content}</span>{segments(view.content, (value) => ({ set_display_content: value }))}<small>{labels.content_hint}</small></section>
    <div className="vr-dashboard-columns">{displayCard("headset")}{displayCard("wrist")}</div>
    {(view.ocr_available || view.vision_available) && <section className="vr-dashboard-card vr-dashboard-tools">
      <header><ScanEye /><h2>{labels.tools}</h2></header>
      <div className="vr-dashboard-tools-grid">
        {view.ocr_available && <>
          {toggle("toggle_ocr", labels.ocr, view.ocr.enabled, !view.ocr.available, !view.ocr.available ? labels.ocr_unavailable : undefined)}
          {toggle("toggle_ocr_gesture", labels.gesture, view.ocr.gesture, !view.ocr.available)}
        </>}
        {view.vision_available && toggle("toggle_vision", labels.vision, view.vision.enabled, !view.vision.enabled && !view.vision.configured, !view.vision.enabled && !view.vision.configured ? labels.vision_unconfigured : view.vision.model || undefined)}
        {action("open_ocr_bindings", <><Settings2 /><span className="vr-dashboard-btn-copy"><span>{labels.bindings}</span></span><ChevronRight /></>, { disabled: !view.vision_available && (!view.ocr_available || !view.ocr.available), className: "vr-dashboard-nav-row" })}
      </div>
    </section>}
  </> : ui.page === "language" ? <>
    <section className="vr-dashboard-language-head">
      {pickerButton({ type: "recognition" }, labels.recognition_language, view.language.recognition, view.language.recognition.options.length === 0 ? (labels.recognition_locked || undefined) : undefined, <Mic />, "vr-dashboard-picker-card")}
      {pickerButton({ type: "mode" }, labels.translation_mode, view.language.mode, undefined, <SlidersHorizontal />, "vr-dashboard-picker-card")}
    </section>
    {translationDisabled && <div className="vr-dashboard-banner muted" role="status"><TriangleAlert /><span>{labels.translation_off_hint}</span></div>}
    <div className={`vr-dashboard-columns vr-dashboard-language-columns${translationDisabled ? " is-muted" : ""}`}>{languageTargets("microphone")}{languageTargets("speaker")}</div>
    <section className="vr-dashboard-card vr-dashboard-presets">
      <Bookmark />
      <h2>{labels.presets}</h2>
      <span className="vr-dashboard-pill">{view.language.presets.options.length} / 5</span>
      {view.language.presets.options.length === 0
        ? <span className="vr-dashboard-presets-empty">{labels.no_presets}</span>
        : button({ type: "picker", value: { type: "apply_preset" } }, <><Check /><span>{labels.apply_preset}</span></>)}
      {action({ save_language_preset: "" }, <><Plus /><span>{labels.save_preset}</span></>, { disabled: !view.language.can_save_preset })}
      {view.language.presets.options.length > 0 && button({ type: "picker", value: { type: "delete_preset" } }, <><Trash2 /><span>{labels.delete}</span></>, { danger: true })}
    </section>
  </> : <section className={`vr-dashboard-card vr-dashboard-osc${!view.osc.enabled ? " is-muted" : ""}`}>
    <header>
      <Radio />
      <h2>{labels.osc_tab}</h2>
      <span className="vr-dashboard-pill mono">{view.osc.endpoint}</span>
      {toggle("toggle_osc", "", view.osc.enabled)}
    </header>
    <div className="vr-dashboard-osc-body">
      <div className="vr-dashboard-osc-toggles">
        {toggle("toggle_osc_original", labels.osc_original, view.osc.original)}
        {toggle("toggle_osc_mute_sync", labels.osc_mute_sync, view.osc.mute_sync)}
      </div>
      <h3>{labels.osc_strategy}</h3>
      {segments(view.osc.strategy, (value) => ({ set_osc_strategy: value }))}
      <p className="vr-dashboard-inline-hint">{labels.osc_hint}</p>
    </div>
  </section>;

  const showMasterOffBanner = !view.error && ui.page === "display" && !view.enabled && !ui.display_editor && !ui.picker && !ui.confirmation;

  return <section className="vr-dashboard vr-surface-panel">
    <header className="vr-dashboard-header">
      <img src="/logos/VRCS_Logo_Small.svg" alt="" />
      <div className="vr-dashboard-title">
        <h1>{labels.title}</h1>
        <span className={`vr-dashboard-status${view.error ? " error" : view.enabled ? " ready" : ""}`}><i aria-hidden="true" />{view.status}</span>
        {(view.save_state === "saving" || view.save_state === "saved") && <span className={`vr-dashboard-save-badge ${view.save_state}`}>{view.save_state === "saving" ? labels.saving : labels.saved}</span>}
      </div>
      <div className="vr-dashboard-global">
        {action("toggle_master", <><Power /><span>{labels.master}</span><span className="vr-dashboard-toggle" aria-hidden="true"><i /></span></>, { active: view.enabled, disabled: Boolean(ui.confirmation), className: "vr-dashboard-master" })}
        {view.network_capture?.visible && action("toggle_network_capture", <><Radar /><span>{view.network_capture.running ? (labels.network_capture_stop || labels.capture_stop) : (labels.network_capture_start || labels.capture_start)}</span></>, { active: view.network_capture.running, disabled: !view.network_capture.available || Boolean(ui.confirmation), className: "vr-dashboard-capture" })}
        {action("toggle_capture", <>{view.capture.running ? <Square /> : <Mic />}<span>{view.capture.running ? labels.capture_stop : labels.capture_start}</span></>, { primary: !view.capture.running, recording: view.capture.running, active: view.capture.running, disabled: !view.capture.available || Boolean(ui.confirmation), className: "vr-dashboard-capture" })}
        {button({ type: "close" }, <X />, { icon: true, label: labels.close })}
      </div>
    </header>
    <nav className="vr-dashboard-nav vr-surface-capsule">
      {button({ type: "page", value: "display" }, <><Monitor /><span>{labels.display_tab}</span></>, { active: ui.page === "display", disabled: Boolean(ui.confirmation) })}
      {button({ type: "page", value: "language" }, <><Languages /><span>{labels.language_tab}</span></>, { active: ui.page === "language", disabled: Boolean(ui.confirmation) })}
      {view.osc_available && button({ type: "page", value: "osc" }, <><Radio /><span>{labels.osc_tab}</span></>, { active: ui.page === "osc", disabled: Boolean(ui.confirmation) })}
    </nav>
    <main className="vr-dashboard-main">
      {view.error && <div className="vr-dashboard-banner error" role="alert"><TriangleAlert /><span>{view.error}</span></div>}
      {showMasterOffBanner && <div className="vr-dashboard-banner warning" role="status"><TriangleAlert /><span>{labels.master_off_banner || labels.subtitle}</span></div>}
      {content}
    </main>
  </section>;
}
