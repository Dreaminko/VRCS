import { useCallback, useEffect, useRef, useState, type PointerEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Hand, Layers3, RotateCcw, StepForward } from "lucide-react";
import { DropdownField } from "../shared/ui/DropdownField";
import { decodeFrame, texturePoint, type PreviewFrame } from "./frame";
import { previewQueue, type PreviewOperation } from "./queue";
import { VrSurface } from "../vr-surface/VrSurface";
import { SurfacePointer } from "../vr-surface/SurfacePointer";
import { SURFACE_SIZE, type SurfaceLayout, type SurfaceState } from "../vr-surface/protocol";
import "./preview.css";

interface Info {
  session: number; live: boolean; panels: string[]; panel: number; scene: string;
  generation: number; revision: number; frame: number; dark: boolean;
  fail_save: boolean; tracking: boolean; mode: string; dragging: boolean; busy: boolean;
  pose: number[][] | null; last_action: string;
  surface: SurfaceState | null;
  closed: boolean;
}
const panels = ["手腕菜单", "SteamVR Dashboard", "手腕对话", "面前字幕", "OCR 阅读器", "视觉问答", "OCR 选区", "OCR 镜像", "OCR 进度", "问答录音 / 确认"];
const scenes = [["normal", "正常"], ["empty", "空内容"], ["long", "长文本"], ["partial", "流式 / 部分结果"], ["expired", "内容过期"], ["busy", "处理中"], ["unavailable", "不可用"], ["error", "错误"], ["timeout", "超时"]];

export function VrPreview() {
  const [info, setInfo] = useState<Info | null>(null);
  const [error, setError] = useState("");
  const [background, setBackground] = useState("checker");
  const [depth, setDepth] = useState(0.8);
  const [rotation, setRotation] = useState([0, 0, 0]);
  const [source, setSource] = useState("Welcome to VRCS.\n欢迎使用 VRCS。");
  const [translation, setTranslation] = useState("操作面板时仍可在游戏中移动。");
  const [stageSize, setStageSize] = useState({ width: 0, height: 0 });
  const stage = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const frame = useRef<PreviewFrame | null>(null);
  const shown = useRef<{ width: number; height: number; panel: number; generation: number; revision: number } | null>(null);
  const queue = useRef<ReturnType<typeof previewQueue> | null>(null);
  const pointer = useRef<{ point: [number, number] | null; buttons: number }>({ point: null, buttons: 0 });
  const pose = useRef({ depth, rotation });
  pose.current = { depth, rotation };

  useEffect(() => {
    const element = stage.current;
    if (!element) return;
    const measure = () => setStageSize({ width: element.clientWidth, height: element.clientHeight });
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    measure();
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    let disposed = false;
    let session = 0;
    async function display(next: Info) {
      if (disposed) return;
      if (next.surface) {
        const [width, height] = SURFACE_SIZE[next.surface.view.kind];
        shown.current = { width, height, panel: next.panel, generation: next.generation, revision: next.revision };
        frame.current = null;
      } else if (frame.current?.sequence !== next.frame) {
        const bytes = await invoke<ArrayBuffer>("vr_preview_frame", { session, sequence: next.frame });
        if (disposed) return;
        const decoded = decodeFrame(bytes);
        if (decoded.generation !== next.generation || decoded.revision !== next.revision || decoded.panel !== next.panel) throw new Error("预览帧已失效");
        const context = canvas.current?.getContext("2d");
        if (context && canvas.current) {
          canvas.current.width = decoded.width;
          canvas.current.height = decoded.height;
          context.putImageData(new ImageData(new Uint8ClampedArray(decoded.pixels), decoded.width, decoded.height), 0, 0);
          frame.current = decoded;
          shown.current = decoded;
        }
      }
      document.documentElement.dataset.theme = next.dark ? "dark" : "light";
      setInfo(next);
    }
    const transport = previewQueue(async (operation) => {
      try {
        const next = await invoke<Info>("vr_preview_update", { session, operation });
        await display(next);
        if (!disposed) setError("");
      } catch (reason) {
        if (!disposed) { pointer.current.buttons = 0; setError(String(reason)); }
      }
    });
    void invoke<Info>("vr_preview_open").then(async (next) => {
      if (disposed) return;
      session = next.session;
      queue.current = transport;
      await display(next);
    }).catch((reason) => { if (!disposed) setError(String(reason)); });
    const timer = window.setInterval(() => { if (queue.current === transport) transport.push({ type: "refresh" }); }, 400);
    const cancel = () => { pointer.current.buttons = 0; transport.push({ type: "cancel" }); };
    const visibility = () => { if (document.hidden) cancel(); };
    window.addEventListener("blur", cancel);
    document.addEventListener("visibilitychange", visibility);
    return () => {
      disposed = true;
      window.clearInterval(timer);
      window.removeEventListener("blur", cancel);
      document.removeEventListener("visibilitychange", visibility);
      transport.close();
      if (session) void invoke("vr_preview_update", { session, operation: { type: "cancel" } }).catch(() => {});
      if (queue.current === transport) queue.current = null;
      frame.current = null;
      shown.current = null;
    };
  }, []);

  function send(operation: PreviewOperation) { queue.current?.push(operation); }
  function cancel() { pointer.current.buttons = 0; send({ type: "cancel" }); }
  function input(point = pointer.current.point, buttons = pointer.current.buttons) {
    const displayed = shown.current;
    if (!displayed) return;
    send({ type: "pointer", input: { panel: displayed.panel, generation: displayed.generation, revision: displayed.revision, point, buttons, ...pose.current } });
  }
  function handlePointer(event: PointerEvent<HTMLElement>) {
    if (event.pointerType !== "mouse" && event.pointerType !== "pen") return;
    const box = event.currentTarget.getBoundingClientRect();
    const displayed = shown.current;
    const point = displayed ? texturePoint(event.clientX - box.left, event.clientY - box.top, box, displayed) : null;
    pointer.current = { point, buttons: event.buttons & 3 };
    if (event.type === "pointerdown") event.currentTarget.setPointerCapture(event.pointerId);
    input();
  }

  const onSurfaceLayout = useCallback((layout: SurfaceLayout) => {
    queue.current?.push({ type: "layout", layout });
  }, []);
  const surface = info?.surface;
  const size = surface ? SURFACE_SIZE[surface.view.kind] : [0, 0];
  const scale = surface ? Math.min(stageSize.width / size[0], stageSize.height / size[1]) : 1;
  const pointerHandlers = {
    onPointerMove: handlePointer, onPointerDown: handlePointer, onPointerUp: handlePointer,
    onPointerLeave: () => { pointer.current.point = null; cancel(); }, onPointerCancel: cancel,
    onLostPointerCapture: () => { if (pointer.current.buttons) cancel(); },
  };

  const ready = Boolean(info);
  return <main className="vr-preview">
    <header className="vr-preview-header">
      <div className="vr-preview-title"><Layers3 size={22} /><div><h1>VR 功能预览</h1><p>{surface ? "React / WebView" : "Native texture"} · 合成姿态</p></div></div>
      <span className={`vr-preview-mode ${info?.live ? "live" : ""}`}>{info?.live ? "LIVE · 操作真实配置与服务" : "MOCK · 仅内存模拟"}</span>
    </header>
    <div className="vr-preview-layout">
      <section className="vr-preview-main">
        <div className="vr-preview-toolbar">
          <DropdownField label="面板" value={String(info?.panel ?? 0)} options={panels.map((label, i) => ({ label, value: String(i) }))} disabled={!ready}
            onChange={(value) => { cancel(); send({ type: "select", panel: Number(value) }); }} />
          <DropdownField label="场景" value={info?.scene ?? "normal"} options={scenes.map(([value, label]) => ({ value, label }))} disabled={!ready}
            onChange={(scene) => { cancel(); send({ type: "scene", scene }); }} />
          <button className="secondary-button" disabled={!ready} onClick={() => send({ type: "advance" })}><StepForward size={16} />推进状态</button>
        </div>
        <div ref={stage} className={`vr-preview-stage ${background}`}>
          <canvas ref={canvas} hidden={Boolean(surface)} aria-label="VR 图层预览，左键点击，右键抓取字幕把手" tabIndex={0}
            {...pointerHandlers}
            onContextMenu={(event) => event.preventDefault()} onKeyDown={(event) => { if (event.key === "Escape") cancel(); }}
            onWheel={(event) => {
              if (!(pointer.current.buttons & 2)) return;
              const next = Math.max(0.2, Math.min(3, pose.current.depth + event.deltaY * 0.001));
              pose.current.depth = next; setDepth(next); input();
            }} />
          {surface && <div className="vr-preview-react-surface" style={{ width: size[0] * scale, height: size[1] * scale, visibility: info?.closed ? "hidden" : "visible" }} tabIndex={0}
            aria-label="React VR 面板预览，左键点击，右键抓取字幕把手" {...pointerHandlers}
            onContextMenu={(event) => event.preventDefault()} onKeyDown={(event) => { if (event.key === "Escape") cancel(); }}
            onWheel={(event) => {
              if (!(pointer.current.buttons & 2)) return;
              const next = Math.max(0.2, Math.min(3, pose.current.depth + event.deltaY * 0.001));
              pose.current.depth = next; setDepth(next); input();
            }}>
            <div className="vr-preview-react-inner" style={{ width: size[0], height: size[1], transform: `scale(${scale})` }}>
              <VrSurface state={surface} onLayout={onSurfaceLayout} />
              {surface.view.kind !== "headset" && surface.view.kind !== "ocr_progress" && <SurfacePointer point={surface.pointer} />}
            </div>
          </div>}
        </div>
        <p className="vr-preview-hint">左键 = Trigger；右键在字幕标题把手上抓取，释放后固定；滚轮调深度；Esc 取消。先移入再按下。</p>
        <output className="vr-preview-output" aria-live="polite">{error || info?.last_action || "正在连接预览窗口…"}</output>
      </section>
      <aside className="vr-preview-options">
        <section><h2>显示</h2>
          <DropdownField label="背景" value={background} options={[{ value: "checker", label: "透明棋盘格" }, { value: "plain", label: "纯色背景" }]} onChange={setBackground} />
          <label className="vr-preview-check"><input type="checkbox" checked={info?.dark ?? false} disabled={!ready} onChange={(e) => send({ type: "dark", enabled: e.target.checked })} />深色面板</label>
        </section>
        <section><h2>字幕位置</h2>
          <div className="vr-preview-actions"><button className="secondary-button" disabled={!ready || info?.busy} onClick={() => send({ type: "place", mode: "wrist" })}><Hand size={15} />回到手腕</button>
            <button className="secondary-button" disabled={!ready || info?.busy} onClick={() => send({ type: "place", mode: "world" })}>移到面前</button></div>
          <p>{info?.dragging ? "抓取中" : info?.busy ? "正在保存锚定模式" : info?.mode === "world" ? "固定在空间" : "固定在手腕"}</p>
          <label className="vr-preview-range">深度 {depth.toFixed(2)} m<input type="range" min="0.2" max="3" step="0.01" value={depth} onChange={(e) => { const next = Number(e.target.value); setDepth(next); pose.current.depth = next; input(); }} /></label>
          {["俯仰", "偏航", "滚转"].map((label, i) => <label className="vr-preview-range" key={label}>{label} {rotation[i]}°<input type="range" min="-180" max="180" step="5" value={rotation[i]} onChange={(e) => { const next = rotation.map((v, index) => index === i ? Number(e.target.value) : v); setRotation(next); pose.current.rotation = next; input(); }} /></label>)}
          <label className="vr-preview-check"><input type="checkbox" checked={info?.tracking ?? true} disabled={!ready} onChange={(e) => send({ type: "tracking", available: e.target.checked })} />控制器追踪有效</label>
          <button className="secondary-button" disabled={!ready} onClick={() => send({ type: "origin_changed" })}>模拟追踪原点变化</button>
          <code className="vr-preview-pose">{info?.pose?.map((row) => row.map((v) => v.toFixed(3)).join("  ")).join("\n")}</code>
          <small>位置为合成 pose；LIVE 只保存锚定模式。</small>
        </section>
        <section><h2>字幕内容</h2>
          <label>原文<textarea value={source} maxLength={8000} onChange={(e) => setSource(e.target.value)} /></label>
          <label>译文<textarea value={translation} maxLength={8000} onChange={(e) => setTranslation(e.target.value)} /></label>
          <button className="secondary-button" disabled={!ready} onClick={() => send({ type: "text", source, translation })}>注入字幕</button>
        </section>
        {!info?.live && <section><h2>模拟执行</h2>
          <label className="vr-preview-check"><input type="checkbox" checked={info?.fail_save ?? false} disabled={!ready} onChange={(e) => send({ type: "save_failure", enabled: e.target.checked })} />模拟保存失败</label>
          <button className="secondary-button" disabled={!ready} onClick={() => { cancel(); send({ type: "reset" }); }}><RotateCcw size={15} />重置模拟</button>
        </section>}
        <small>截图、设备绑定、真实追踪和游戏输入共存需要头显验证。</small>
      </aside>
    </div>
  </main>;
}
