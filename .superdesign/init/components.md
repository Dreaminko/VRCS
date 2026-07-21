# Shared UI primitives

The project has no standalone component library. Reusable primitives are co-located in `apps/desktop/src/App.tsx`.

## Metric

- Path: `apps/desktop/src/App.tsx`
- Props: `label`, `value`
- Description: Compact status metric used in the live dashboard.

```tsx
function Metric({ label, value }: { label: string; value: string }) {
  return <div className="metric"><span>{label}</span><strong title={value}>{value}</strong></div>;
}
```

## SubtitleList

- Path: `apps/desktop/src/App.tsx`
- Props: `subtitles`, `onSelect`, `empty`
- Description: Reusable transcript/history list with timestamp, text, and language.

```tsx
function SubtitleList({ subtitles, onSelect, empty }: { subtitles: Subtitle[]; onSelect: (text: string) => void; empty: string }) {
  if (!subtitles.length) return <div className="empty">{empty}</div>;
  return <div className="subtitle-list">{subtitles.map((subtitle, index) => (
    <article key={subtitle.id ?? `${subtitle.created_at}-${index}`} onMouseUp={() => void onSelect(subtitle.text)}>
      <time>{timestamp(subtitle.created_at)}</time><p>{subtitle.text}</p><em>{subtitle.language ?? "—"}</em>
    </article>
  ))}</div>;
}
```

## Select

- Path: `apps/desktop/src/App.tsx`
- Props: `label`, `value`, `values`, `onChange`
- Description: Labeled native select used by ASR settings.

```tsx
function Select({ label, value, values, onChange }: { label: string; value: string; values: string[]; onChange: (value: string) => void }) {
  return <label><span>{label}</span><select value={value} onChange={(event) => onChange(event.target.value)}>{values.map((item) => <option key={item}>{item}</option>)}</select></label>;
}
```

## SettingsPanel

- Path: `apps/desktop/src/App.tsx`
- Props: `settings`, `onSave`, `disabled`, `modelStatus`
- Description: Whisper model and runtime configuration form.

```tsx
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
```

## AudioPanel

- Path: `apps/desktop/src/App.tsx`
- Props: `settings`, `devices`, `running`, `onRefresh`, `onSave`
- Description: Loopback output device picker and capture state.

```tsx
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
```

## LookupPanel

- Path: `apps/desktop/src/App.tsx`
- Props: `lookup`, `onClose`
- Description: Fixed dictionary side panel with definitions, context, and Anki action.

```tsx
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
```

