import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { useAppTheme } from "../app/useAppTheme";
import "./capture-image.css";
import { imageRegion, type Point, type Region } from "./geometry";

export function CaptureImageWindow() {
  useAppTheme();
  const { t } = useTranslation();
  const id = Number(new URLSearchParams(window.location.search).get("id"));
  const image = useRef<HTMLImageElement>(null);
  const start = useRef<Point | null>(null);
  const pending = useRef(false);
  const [url, setUrl] = useState<string | null>(null);
  const [selection, setSelection] = useState<Region | null>(null);
  const [error, setError] = useState("");
  const cancel = useCallback(() => {
    void invoke("desktop_capture_cancel", { id }).catch((reason: unknown) => setError(String(reason)));
  }, [id]);
  useEffect(() => {
    let disposed = false;
    let objectUrl: string | null = null;
    void invoke<ArrayBuffer>("desktop_capture_frame", { id }).then((bytes) => {
      if (disposed) return;
      objectUrl = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: "image/png" }));
      setUrl(objectUrl);
    }).catch((reason: unknown) => { if (!disposed) setError(String(reason)); });
    return () => { disposed = true; if (objectUrl) URL.revokeObjectURL(objectUrl); };
  }, [id]);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); cancel(); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [cancel]);
  const point = (clientX: number, clientY: number): Point | null => {
    const element = image.current;
    if (!element || !element.naturalWidth) return null;
    const rect = element.getBoundingClientRect();
    return { x: (clientX - rect.left) * element.naturalWidth / rect.width, y: (clientY - rect.top) * element.naturalHeight / rect.height };
  };
  return <main className="capture-image-window" onContextMenu={(event) => { event.preventDefault(); cancel(); }}>
    <header><p>{t("ocrWindow.selectionHint")}</p><button onClick={cancel}>{t("common.cancel")}</button></header>
    {error && <p role="alert">{error}</p>}
    <div className="capture-image-viewport">
      {url && <div className="capture-image-bitmap"
        onPointerDown={(event) => {
          if (event.button !== 0 || pending.current) return;
          event.preventDefault();
          start.current = point(event.clientX, event.clientY);
          setSelection(null);
          event.currentTarget.setPointerCapture(event.pointerId);
        }}
        onPointerMove={(event) => {
          const end = point(event.clientX, event.clientY);
          const element = image.current;
          if (start.current && end && element) setSelection(imageRegion(start.current, end, element.naturalWidth, element.naturalHeight));
        }}
        onPointerCancel={() => { start.current = null; setSelection(null); }}
        onPointerUp={(event) => {
          const end = point(event.clientX, event.clientY);
          const element = image.current;
          const region = start.current && end && element ? imageRegion(start.current, end, element.naturalWidth, element.naturalHeight) : null;
          start.current = null;
          if (!region || pending.current) return;
          setSelection(region);
          pending.current = true;
          void invoke("desktop_capture_select", { id, ...region }).catch((reason: unknown) => { pending.current = false; setError(String(reason)); });
        }}>
        <img ref={image} src={url} alt={t("visionWindow.capture")} draggable={false} />
        {selection && image.current && <div className="capture-image-region" style={{
          left: `${selection.left / image.current.naturalWidth * 100}%`, top: `${selection.top / image.current.naturalHeight * 100}%`,
          width: `${selection.width / image.current.naturalWidth * 100}%`, height: `${selection.height / image.current.naturalHeight * 100}%`,
        }} />}
      </div>}
    </div>
  </main>;
}
