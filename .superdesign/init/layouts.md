# Shared layout

## App shell

- Path: `apps/desktop/src/App.tsx`
- Description: Fixed sidebar, page header, conditional content view, and optional dictionary drawer.
- Full source:

```tsx
import { useCallback, useEffect, useState } from "react";
import { coreApi, WS_URL } from "./api";
import type {
  AudioDevice,
  ConnectionState,
  DictionaryEntry,
  Health,
  Settings,
  Subtitle,
} from "./types";

type Page = "live" | "history" | "asr" | "audio";

const pageNames: Record<Page, string> = {
  live: "实时字幕",
  history: "字幕历史",
  asr: "识别设置",
  audio: "音频设备",
};

function timestamp(value: string): string {
  return new Intl.DateTimeFormat("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(new Date(value));
}

function App() {
  const [page, setPage] = useState<Page>("live");
  const [connection, setConnection] = useState<ConnectionState>("connecting");
  const [health, setHealth] = useState<Health | null>(null);
  const [subtitles, setSubtitles] = useState<Subtitle[]>([]);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [lookup, setLookup] = useState<{ term: string; context: string; entries: DictionaryEntry[] } | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [nextHealth, nextSettings, history] = await Promise.all([
        coreApi.health(),
        coreApi.settings(),
        coreApi.subtitles(),
      ]);
      setHealth(nextHealth);
      setSettings(nextSettings);
      setSubtitles(history);
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "无法连接 Core 服务");
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void coreApi.health().then(setHealth).catch(() => setHealth(null)), 2500);
    return () => window.clearInterval(timer);
  }, [refresh]);

  useEffect(() => {
    let socket: WebSocket | null = null;
    let retry: number | null = null;
    let closed = false;
    const connect = () => {
      setConnection("connecting");
      socket = new WebSocket(WS_URL);
      socket.onopen = () => setConnection("connected");
      socket.onmessage = (event) => {
        const message = JSON.parse(String(event.data)) as { type: string; subtitle?: Subtitle };
        if (message.type === "subtitle" && message.subtitle) {
          setSubtitles((current) => [message.subtitle!, ...current].slice(0, 500));
        }
      };
      socket.onclose = () => {
        setConnection("disconnected");
        if (!closed) retry = window.setTimeout(connect, 1500);
      };
    };
    connect();
    return () => {
      closed = true;
      if (retry !== null) window.clearTimeout(retry);
      socket?.close();
    };
  }, []);

  const loadDevices = async () => {
    try {
      setDevices(await coreApi.devices());
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "设备枚举失败");
    }
  };

  useEffect(() => {
    if (page === "audio") void loadDevices();
  }, [page]);

  const toggleCapture = async () => {
    try {
      if (health?.capture_running) await coreApi.stop();
      else await coreApi.start(settings?.audio_device_id ?? null);
      setHealth(await coreApi.health());
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "操作失败");
    }
  };

  const saveSettings = async (next: Settings) => {
    try {
      setSettings(await coreApi.saveSettings(next));
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "设置保存失败");
    }
  };

  const selectWord = async (context: string) => {
    const term = window.getSelection()?.toString().trim().replace(/^[\s.,!?;:]+|[\s.,!?;:]+$/g, "");
    if (!term) return;
    try {
      setLookup({ term, context, entries: await coreApi.lookup(term) });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "查词失败");
    }
  };

  return (
    <div className="shell">
      <aside>
        <div className="brand"><span>V</span><div>VRCS<small>LOCAL SUBTITLES</small></div></div>
        <nav>
          {(Object.keys(pageNames) as Page[]).map((item) => (
            <button className={page === item ? "active" : ""} onClick={() => setPage(item)} key={item}>
              {pageNames[item]}
            </button>
          ))}
        </nav>
        <div className={`connection ${connection}`}><i />Core {connection === "connected" ? "已连接" : "未连接"}</div>
      </aside>

      <main>
        <header>
          <div><p className="eyebrow">VRCHAT LANGUAGE MINING</p><h1>{pageNames[page]}</h1></div>
          <button className={`capture ${health?.capture_running ? "stop" : ""}`} onClick={() => void toggleCapture()}>
            {health?.capture_running ? "停止转写" : "开始转写"}
          </button>
        </header>

        {error && <div className="error" role="alert">{error}<button onClick={() => setError(null)}>×</button></div>}

        {page === "live" && (
          <section>
            <div className="metrics">
              <Metric label="状态" value={health?.capture_running ? "正在聆听" : "等待开始"} />
              <Metric label="音频设备" value={health?.audio_device?.name ?? "系统默认输出"} />
              <Metric label="Whisper" value={`${settings?.asr.model ?? "small"} · ${health?.asr_status ?? "未知"}`} />
              <Metric label="语言" value={settings?.asr.language === "auto" ? "自动检测" : settings?.asr.language ?? "自动检测"} />
            </div>
            <div className="panel transcript-panel">
              <div className="panel-title"><h2>最近字幕</h2><span>选中文字即可查词</span></div>
              <SubtitleList subtitles={subtitles.slice(0, 12)} onSelect={selectWord} empty="开始转写后，字幕会显示在这里。" />
            </div>
          </section>
        )}

        {page === "history" && (
          <section className="panel">
            <div className="panel-title"><h2>字幕历史</h2><span>本地保留最近 500 条</span></div>
            <SubtitleList subtitles={subtitles} onSelect={selectWord} empty="还没有字幕记录。" />
          </section>
        )}

        {page === "asr" && settings && (
          <SettingsPanel settings={settings} onSave={saveSettings} disabled={health?.capture_running ?? false} modelStatus={health?.asr_status ?? "unknown"} />
        )}

        {page === "audio" && settings && (
          <AudioPanel settings={settings} devices={devices} running={health?.capture_running ?? false} onRefresh={loadDevices} onSave={saveSettings} />
        )}
      </main>

      {lookup && <LookupPanel lookup={lookup} onClose={() => setLookup(null)} />}
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return <div className="metric"><span>{label}</span><strong title={value}>{value}</strong></div>;
}

function SubtitleList({ subtitles, onSelect, empty }: { subtitles: Subtitle[]; onSelect: (text: string) => void; empty: string }) {
  if (!subtitles.length) return <div className="empty">{empty}</div>;
  return <div className="subtitle-list">{subtitles.map((subtitle, index) => (
    <article key={subtitle.id ?? `${subtitle.created_at}-${index}`} onMouseUp={() => void onSelect(subtitle.text)}>
      <time>{timestamp(subtitle.created_at)}</time><p>{subtitle.text}</p><em>{subtitle.language ?? "—"}</em>
    </article>
  ))}</div>;
}

function SettingsPanel({ settings, onSave, disabled, modelStatus }: { settings: Settings; onSave: (value: Settings) => void; disabled: boolean; modelStatus: string }) {
  const [draft, setDraft] = useState(settings);
  useEffect(() => setDraft(settings), [settings]);
  const update = (key: keyof Settings["asr"], value: string) => setDraft({ ...draft, asr: { ...draft.asr, [key]: value } });
  return <section className="panel form-panel">
    <div className="panel-title"><h2>Whisper 设置</h2><span>模型状态：{modelStatus}{disabled ? " · 停止转写后可修改" : ""}</span></div>
    <div className="form-grid">
      <Select label="模型" value={draft.asr.model} values={["tiny", "base", "small", "medium", "large-v3"]} onChange={(v) => update("model", v)} />
      <Select label="语言" value={draft.asr.language} values={["auto", "en", "ja", "zh", "ko", "es", "fr", "de"]} onChange={(v) => update("language", v)} />
      <Select label="设备" value={draft.asr.device} values={["auto", "cpu", "cuda"]} onChange={(v) => update("device", v)} />
      <Select label="计算类型" value={draft.asr.compute_type} values={["int8", "float16", "int8_float16"]} onChange={(v) => update("compute_type", v)} />
    </div>
    <button className="primary" disabled={disabled} onClick={() => void onSave(draft)}>保存设置</button>
  </section>;
}

function Select({ label, value, values, onChange }: { label: string; value: string; values: string[]; onChange: (value: string) => void }) {
  return <label><span>{label}</span><select value={value} onChange={(event) => onChange(event.target.value)}>{values.map((item) => <option key={item}>{item}</option>)}</select></label>;
}

function AudioPanel({ settings, devices, running, onRefresh, onSave }: { settings: Settings; devices: AudioDevice[]; running: boolean; onRefresh: () => void; onSave: (value: Settings) => void }) {
  return <section className="panel form-panel">
    <div className="panel-title"><h2>系统输出设备</h2><span>捕获测试：{running ? "正在接收" : "未运行"}</span><button className="text-button" onClick={() => void onRefresh()}>重新扫描</button></div>
    <div className="device-list">
      <label className={settings.audio_device_id === null ? "chosen" : ""}>
        <input type="radio" checked={settings.audio_device_id === null} onChange={() => void onSave({ ...settings, audio_device_id: null })} />
        <div><strong>系统默认输出</strong><span>跟随 Windows 默认设备</span></div>
      </label>
      {devices.map((device) => <label className={settings.audio_device_id === device.id ? "chosen" : ""} key={device.id}>
        <input type="radio" checked={settings.audio_device_id === device.id} onChange={() => void onSave({ ...settings, audio_device_id: device.id })} />
        <div><strong>{device.name}</strong><span>{device.sample_rate} Hz · {device.channels} 声道{device.is_default ? " · 默认" : ""}</span></div>
      </label>)}
    </div>
    {!devices.length && <div className="empty small">未发现设备。请确认已安装音频依赖并重新扫描。</div>}
  </section>;
}

function LookupPanel({ lookup, onClose }: { lookup: { term: string; context: string; entries: DictionaryEntry[] }; onClose: () => void }) {
  const [message, setMessage] = useState("");
  const entry = lookup.entries[0];
  const add = async () => {
    if (!entry) return;
    try {
      const result = await coreApi.createCard(lookup.term, entry.definition, lookup.context);
      setMessage(`已创建卡片 #${result.note_id}`);
    } catch (reason) {
      setMessage(reason instanceof Error ? reason.message : "制卡失败");
    }
  };
  return <div className="lookup-panel">
    <button className="close" onClick={onClose}>×</button><p className="eyebrow">DICTIONARY</p><h2>{lookup.term}</h2>
    {lookup.entries.length ? lookup.entries.map((item) => <div className="definition" key={`${item.term}-${item.language}`}><span>{item.language}</span><p>{item.definition}</p></div>) : <p className="muted">内置测试词典中暂无释义。</p>}
    <blockquote>{lookup.context}</blockquote>
    <button className="primary full" disabled={!entry} onClick={() => void add()}>添加到 Anki</button>
    {message && <small className="feedback">{message}</small>}
  </div>;
}

export default App;
```

