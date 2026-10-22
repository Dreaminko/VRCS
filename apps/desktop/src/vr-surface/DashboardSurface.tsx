import type { ReactNode } from "react";
import { ArrowDown, ArrowUp, Check, ChevronLeft, ChevronRight, Eye, Hand, Languages, Mic, Minus, Monitor, Pin, Plus, Power, Radio, RotateCcw, ScanEye, Settings2, Square, Trash2, X } from "lucide-react";
import type { VrDashboardChoice, VrDashboardViewModel } from "../integrations/vr-overlay-native";
import type { VrOverlayPositionField } from "../settings/vr-overlay-settings";
import dashboardThumbnail from "../../src-tauri/icons/128x128@2x.png";
import { DASHBOARD_PICKER_PAGE_SIZE, dashboardPickerChoice, type DashboardAction, type DashboardCommand, type DashboardPicker, type DashboardUi, type DisplayKind, type TargetGroup } from "./dashboard-protocol";
import "./dashboard.css";

type ButtonOptions = { active?: boolean; disabled?: boolean; icon?: boolean; primary?: boolean; danger?: boolean; label?: string; key?: string };

export function DashboardThumbnail() {
  return <img className="vr-dashboard-thumbnail" src={dashboardThumbnail} alt="VRCS" />;
}

export function DashboardSurface({ dashboard: view, ui, hovered, pressed }: {
  dashboard: VrDashboardViewModel; ui: DashboardUi; hovered: number | null; pressed: number | null;
}) {
  const labels = view.labels;
  let region = 0;
  const button = (command: DashboardCommand, children: ReactNode, options: ButtonOptions = {}) => {
    const id = region++;
    return <button key={options.key} type="button" tabIndex={-1} data-vr-region={id} data-vr-command={JSON.stringify(command)} disabled={options.disabled}
      aria-label={options.label} aria-pressed={options.active} className={`vr-dashboard-button${options.icon ? " icon" : ""}${options.active ? " active" : ""}${options.primary ? " primary" : ""}${options.danger ? " danger" : ""}${hovered === id ? " hovered" : ""}${pressed === id ? " pressed" : ""}`}>
      {children}
    </button>;
  };
  const action = (value: DashboardAction, children: ReactNode, options: ButtonOptions = {}) => button({ type: "action", value }, children, options);
  const toggle = (value: DashboardAction, label: string, enabled: boolean, disabled = false) => action(value, <><span>{label}</span><span className="vr-dashboard-toggle" aria-hidden="true"><i /></span></>, { active: enabled, disabled });
  const segments = (choice: VrDashboardChoice, toAction: (value: string) => DashboardAction) => <div className="vr-dashboard-segments">
    {choice.options.map((option) => action(toAction(option.value), option.label, { active: choice.value === option.value || choice.value === option.label, key: option.value }))}
  </div>;
  const stepper = (title: string, value: string, down: DashboardAction, up: DashboardAction) => <div className="vr-dashboard-stepper">
    <span>{title}</span><div>{action(down, <Minus />, { icon: true, label: `${title} −` })}<strong>{value}</strong>{action(up, <Plus />, { icon: true, label: `${title} +` })}</div>
  </div>;
  const pickerButton = (picker: DashboardPicker, label: string, choice: VrDashboardChoice) => button({ type: "picker", value: picker }, <><span>{label}</span><strong>{choice.value}</strong><ChevronRight /></>, { disabled: !choice.options.length });

  const displayCard = (kind: DisplayKind) => {
    const display = view[kind];
    const wrist = kind === "wrist";
    return <section className="vr-dashboard-card" key={kind}>
      <header>{wrist ? <Hand /> : <Monitor />}<h2>{wrist ? labels.wrist : labels.headset}</h2>{toggle(wrist ? "toggle_wrist" : "toggle_headset", "", display.enabled)}</header>
      <div className="vr-dashboard-card-options">{wrist ? <><span>{labels.hand}</span>{segments(view.wrist.hand, (value) => ({ set_wrist_hand: value }))}</> : <p>{labels.headset_hint}</p>}</div>
      <div className="vr-dashboard-steppers">{stepper(labels.width, display.width, wrist ? "wrist_width_down" : "headset_width_down", wrist ? "wrist_width_up" : "headset_width_up")}{stepper(labels.opacity, display.opacity, wrist ? "wrist_opacity_down" : "headset_opacity_down", wrist ? "wrist_opacity_up" : "headset_opacity_up")}</div>
      <div className="vr-dashboard-card-actions">{button({ type: "position", value: kind }, <><Settings2 />{labels.position}</>)}{action(wrist ? "preview_wrist" : "preview_headset", <><Eye />{display.preview_visible ? labels.hide_preview : labels.preview}</>, { active: display.preview_visible })}</div>
    </section>;
  };

  const languageTargets = (group: TargetGroup) => {
    const targets = view.language[`${group}_targets`];
    return <section className="vr-dashboard-card" key={group}>
      <h2>{group === "microphone" ? labels.microphone_language : labels.speaker_language}</h2>
      <div className="vr-dashboard-targets">{targets.map((choice, index) => <div className="vr-dashboard-target" key={index}>
        {pickerButton({ type: "target", value: { group, index } }, `${index + 1}`, choice)}
        {action({ move_target: { group, index, offset: -1 } }, <ArrowUp />, { icon: true, disabled: index === 0, label: "↑" })}
        {action({ move_target: { group, index, offset: 1 } }, <ArrowDown />, { icon: true, disabled: index + 1 >= targets.length, label: "↓" })}
        {action({ delete_target: { group, index } }, <Trash2 />, { icon: true, danger: true, disabled: targets.length <= 1, label: labels.delete })}
      </div>)}</div>
      {action({ add_target: group }, <><Plus />{labels.add_target}</>, { disabled: !view.language.can_add || targets.length >= 3 })}
    </section>;
  };

  const positionEditor = (kind: DisplayKind) => {
    const display = view[kind];
    const fieldGroup = (rotation: boolean) => <div className="vr-dashboard-position-grid">{display.position.filter((item) => ["pitch_deg", "yaw_deg", "roll_deg"].includes(item.field) === rotation).map((item) => {
      const adjust = (direction: -1 | 1) => ({ adjust_display_position: { kind, field: item.field as VrOverlayPositionField, direction } });
      return <div className="vr-dashboard-stepper" key={item.field}><span>{item.label}</span><div>{action(adjust(-1), <Minus />, { icon: true, disabled: !item.can_decrease, label: `${item.label} −` })}<strong>{item.value}</strong>{action(adjust(1), <Plus />, { icon: true, disabled: !item.can_increase, label: `${item.label} +` })}</div></div>;
    })}</div>;
    return <section className="vr-dashboard-card vr-dashboard-position">
      <header><Settings2 /><h2>{kind === "wrist" ? labels.wrist : labels.headset} · {labels.position}</h2>{action(kind === "wrist" ? "preview_wrist" : "preview_headset", <><Eye />{display.preview_visible ? labels.hide_preview : labels.preview}</>, { active: display.preview_visible })}{button({ type: "close" }, <X />, { icon: true, label: labels.close })}</header>
      <p>{labels.position_hint}</p>{fieldGroup(false)}<h3>{labels.rotation}<small>{labels.rotation_hint}</small></h3>{fieldGroup(true)}
      <div className="vr-dashboard-position-actions">{action({ reset_display_position: kind }, <><RotateCcw />{labels.reset_position}</>)}{kind === "wrist" && <>{action("recenter_dialogue", <><Pin />{labels.recenter_dialogue}</>)}{action("attach_dialogue", <><Hand />{labels.attach_dialogue}</>, { active: view.wrist.anchor_mode === "wrist" })}</>}</div>
    </section>;
  };

  const picker = ui.picker;
  const choice = picker ? dashboardPickerChoice(view, picker) : undefined;
  const pickerTitle = picker?.type === "recognition" ? labels.recognition_language : picker?.type === "mode" ? labels.translation_mode : picker?.type === "target" ? labels.translation_languages : picker?.type === "delete_preset" ? labels.delete : labels.apply_preset;
  const content = ui.confirmation ? <section className="vr-dashboard-card vr-dashboard-confirmation">
    <Trash2 /><h2>{labels.confirm_delete}</h2><strong>{ui.confirmation.label}</strong><p>{labels.delete_hint}</p><div>{button({ type: "cancel" }, labels.cancel)}{button({ type: "confirm" }, labels.delete, { danger: true })}</div>
  </section> : choice ? <section className="vr-dashboard-card vr-dashboard-picker">
    <header><Languages /><h2>{pickerTitle}</h2><span>{ui.picker_page + 1} / {Math.max(1, Math.ceil(choice.options.length / DASHBOARD_PICKER_PAGE_SIZE))}</span>{button({ type: "picker_page", value: -1 }, <ChevronLeft />, { icon: true, disabled: ui.picker_page === 0, label: "←" })}{button({ type: "picker_page", value: 1 }, <ChevronRight />, { icon: true, disabled: (ui.picker_page + 1) * DASHBOARD_PICKER_PAGE_SIZE >= choice.options.length, label: "→" })}{button({ type: "close" }, <X />, { icon: true, label: labels.close })}</header>
    <div className="vr-dashboard-picker-options">{choice.options.slice(ui.picker_page * DASHBOARD_PICKER_PAGE_SIZE, (ui.picker_page + 1) * DASHBOARD_PICKER_PAGE_SIZE).map((option, index) => button({ type: "select", value: ui.picker_page * DASHBOARD_PICKER_PAGE_SIZE + index }, option.label, { active: choice.value === option.value || choice.value === option.label, key: option.value }))}</div>
  </section> : ui.display_editor ? positionEditor(ui.display_editor) : ui.page === "display" ? <>
    <section className="vr-dashboard-content-choice"><span>{labels.content}</span>{segments(view.content, (value) => ({ set_display_content: value }))}<small>{labels.content_hint}</small></section>
    <div className="vr-dashboard-columns">{displayCard("headset")}{displayCard("wrist")}</div>
    <section className="vr-dashboard-card vr-dashboard-tools"><div><ScanEye /><h2>{labels.tools}</h2></div><div>{toggle("toggle_ocr", labels.ocr, view.ocr.enabled, !view.ocr_available || !view.ocr.available)}{toggle("toggle_ocr_gesture", labels.gesture, view.ocr.gesture, !view.ocr_available || !view.ocr.available)}{toggle("toggle_vision", labels.vision, view.vision.enabled, !view.vision_available || !view.vision.enabled && !view.vision.configured)}{action("open_ocr_bindings", <><Settings2 />{labels.bindings}</>, { disabled: !view.vision_available && (!view.ocr_available || !view.ocr.available) })}</div>{(!view.ocr_available || !view.ocr.available || view.vision_available && !view.vision.configured) && <small>{!view.ocr_available || !view.ocr.available ? labels.ocr_unavailable : labels.vision_unconfigured}</small>}</section>
  </> : ui.page === "language" ? <>
    <section className="vr-dashboard-language-head">{pickerButton({ type: "recognition" }, labels.recognition_language, view.language.recognition)}{pickerButton({ type: "mode" }, labels.translation_mode, view.language.mode)}</section>
    <div className="vr-dashboard-columns vr-dashboard-language-columns">{languageTargets("microphone")}{languageTargets("speaker")}</div>
    <section className="vr-dashboard-card vr-dashboard-presets"><h2>{labels.presets}</h2>{button({ type: "picker", value: { type: "apply_preset" } }, labels.apply_preset, { disabled: !view.language.presets.options.length })}{action({ save_language_preset: "" }, <><Plus />{labels.save_preset}</>, { disabled: !view.language.can_save_preset })}{button({ type: "picker", value: { type: "delete_preset" } }, <><Trash2 />{labels.delete}</>, { danger: true, disabled: !view.language.presets.options.length })}</section>
  </> : <section className="vr-dashboard-card vr-dashboard-osc"><header><Radio /><h2>{labels.osc_tab}</h2><span>{view.osc.endpoint}</span></header>{toggle("toggle_osc", labels.osc_enabled, view.osc.enabled)}{toggle("toggle_osc_original", labels.osc_original, view.osc.original)}{toggle("toggle_osc_mute_sync", labels.osc_mute_sync, view.osc.mute_sync)}<h3>{labels.osc_strategy}</h3>{segments(view.osc.strategy, (value) => ({ set_osc_strategy: value }))}<p>{labels.osc_hint}</p></section>;

  return <section className="vr-dashboard vr-surface-panel">
    <header className="vr-dashboard-header"><img src="/logos/VRCS_Logo_Small.svg" alt="" /><div><h1>{labels.title}</h1><p>{labels.subtitle}</p></div>{action("toggle_capture", view.capture.running ? <Square /> : <Mic />, { icon: true, primary: true, active: view.capture.running, disabled: !view.capture.available || Boolean(ui.confirmation), label: view.capture.running ? labels.capture_stop : labels.capture_start })}</header>
    <nav className="vr-dashboard-nav vr-surface-capsule">{button({ type: "page", value: "display" }, <><Monitor />{labels.display_tab}</>, { active: ui.page === "display", disabled: Boolean(ui.confirmation) })}{button({ type: "page", value: "language" }, <><Languages />{labels.language_tab}</>, { active: ui.page === "language", disabled: Boolean(ui.confirmation) })}{view.osc_available && button({ type: "page", value: "osc" }, <><Radio />{labels.osc_tab}</>, { active: ui.page === "osc", disabled: Boolean(ui.confirmation) })}{action("toggle_master", <><Power />{labels.master}{view.enabled && <Check />}</>, { active: view.enabled, disabled: Boolean(ui.confirmation) })}</nav>
    <main className="vr-dashboard-main">{content}</main>
    <footer className="vr-dashboard-footer"><span className={view.error ? "vr-surface-error" : undefined}>{view.error || view.status}</span><span>{view.save_state === "saving" ? labels.saving : view.save_state === "saved" ? labels.saved : ""}</span></footer>
  </section>;
}
