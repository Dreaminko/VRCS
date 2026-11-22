import { useEffect, useId, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { Keyboard } from "lucide-react";
import { useTranslation } from "react-i18next";
import { captureShortcut, isShortcutModifier, shortcutModifiers } from "./shortcut";

// Serialize transitions so an old field's cleanup cannot resume a new recorder.
let recordingTransition: Promise<void> = Promise.resolve();
function setNativeRecording(recording: boolean): Promise<void> {
  const transition = recordingTransition.then(async () => {
    if (isTauri()) await invoke<void>("set_shortcut_recording", { recording });
  });
  recordingTransition = transition.catch(() => undefined);
  return transition;
}

export function ShortcutField({ label, value, disabled = false, onChange }: {
  label: string;
  value: string;
  disabled?: boolean;

  onChange: (shortcut: string) => void;
}) {
  const { t } = useTranslation();
  const id = useId();
  const [recording, setRecording] = useState(false);
  const [ready, setReady] = useState(false);
  const [preview, setPreview] = useState("");
  const [error, setError] = useState("");
  const current = useRef({ value, onChange });
  useEffect(() => { current.current = { value, onChange }; }, [value, onChange]);
  useEffect(() => { setRecording(false); }, [value]);

  useEffect(() => {
    if (!recording || disabled) {
      setRecording(false);
      return;
    }
    let active = true;
    let listening = false;
    let candidate: string | null = null;
    const pressed = new Set<string>();
    const finish = (shortcut?: string) => {
      if (!active) return;
      active = false;
      setRecording(false);
      if (shortcut && shortcut !== current.current.value) current.current.onChange(shortcut);
    };
    const keydown = (event: KeyboardEvent) => {
      if (!active) return;
      if (event.key === "Tab") { finish(); return; }
      event.preventDefault();
      event.stopImmediatePropagation();
      if (event.key === "Escape") { finish(); return; }
      if (!listening || event.repeat) return;
      pressed.add(event.code || event.key);
      if (isShortcutModifier(event.key)) {
        if (!candidate) setPreview(shortcutModifiers(event).join("+"));
        if (event.metaKey || event.key === "AltGraph") setError("unsupported");
        return;
      }
      const shortcut = captureShortcut(event);
      const mainKeys = [...pressed].filter((code) => !/^(Control|Alt|Shift|Meta)(Left|Right)$/.test(code));
      if (!shortcut || mainKeys.length > 1) {
        candidate = null;
        setError("unsupported");
        setPreview(shortcutModifiers(event).join("+"));
        return;
      }
      candidate = shortcut;
      setError("");
      setPreview(shortcut);
    };
    const keyup = (event: KeyboardEvent) => {
      if (!active) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      pressed.delete(event.code || event.key);
      if (!listening) return;
      // Wait for release before registration, so the recorded press cannot capture the screen.
      if (candidate && !event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey && !pressed.size) {
        finish(candidate);
      } else if (!candidate) setPreview(shortcutModifiers(event).join("+"));
    };
    const blur = () => finish();
    const visibility = () => { if (document.hidden) finish(); };
    window.addEventListener("keydown", keydown, true);
    window.addEventListener("keyup", keyup, true);
    window.addEventListener("blur", blur);
    document.addEventListener("visibilitychange", visibility);
    void setNativeRecording(true).then(() => {
      if (!active) return;
      listening = true;
      setReady(true);
    }).catch(() => {
      if (!active) return;
      setError("startFailed");
      finish();
    });
    return () => {
      active = false;
      window.removeEventListener("keydown", keydown, true);
      window.removeEventListener("keyup", keyup, true);
      window.removeEventListener("blur", blur);
      document.removeEventListener("visibilitychange", visibility);
      void setNativeRecording(false).catch((error) => {
        console.error("Could not restore desktop shortcuts", error);
      });
    };
  }, [recording, disabled]);

  const display = recording ? preview : value;
  const status = error || (recording ? (ready ? "listeningHint" : "preparing") : null);
  return <div className="field shortcut-field">
    <span id={`${id}-label`}>{label}</span>
    <button type="button" className={`shortcut-recorder${recording ? " recording" : ""}`}
      disabled={disabled} aria-labelledby={`${id}-label ${id}-preview`} aria-describedby={status ? `${id}-hint` : undefined}
      aria-pressed={recording}
      onClick={() => {
        if (recording) { setRecording(false); return; }
        setPreview(""); setError(""); setReady(false); setRecording(true);
      }}
      onBlur={() => setRecording(false)}>
      <Keyboard size={17} aria-hidden="true" />
      <span className="shortcut-preview" id={`${id}-preview`}>
        {display ? display.split("+").map((key, index) => <span className="shortcut-key" key={index}>
          {index > 0 && <span aria-hidden="true">+</span>}<kbd>{key.trim()}</kbd>
        </span>) : t(`settings.shortcut.${recording ? (ready ? "listening" : "preparing") : "unset"}`)}
      </span>
      {recording && <span className="shortcut-recorder-action" aria-hidden="true">{t("settings.shortcut.cancel")}</span>}
    </button>
    {status && <small id={`${id}-hint`} role={error ? "alert" : "status"}>{t(`settings.shortcut.${status}`)}</small>}
  </div>;
}
