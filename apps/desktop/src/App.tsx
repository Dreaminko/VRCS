import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties } from "react";
import {
  CalendarDays,
  BookOpen,
  Check,
  ChevronDown,
  Clock3,
  History,
  Languages,
  MessageSquare,
  MessageSquareText,
  Mic,
  Minus,
  PanelLeftClose,
  PanelLeftOpen,
  Plus,
  PlusCircle,
  RefreshCw,
  Shrink,
  SlidersHorizontal,
  Square,
  Trash2,
  Upload,
  Volume2,
  X,
} from "lucide-react";
import { coreApi, WS_URL } from "./api";
import { isTauri } from "@tauri-apps/api/core";
import {
  COMPACT_WINDOW_SIZE,
  compactWindowSize,
  subtitleForCompactView,
} from "./compact-mode";
import { conversationId, groupConversations } from "./conversations";
import type { SubtitleConversation } from "./conversations";
import { definitionGlosses, groupDictionaryEntries } from "./dictionary";
import { isLookupAnchorVisible, LOOKUP_POPOVER_HEIGHT, placeLookupPopover } from "./popover-placement";
import type { LookupAnchor } from "./popover-placement";
import type {
  AudioDevice,
  ConnectionState,
  DictionaryEntry,
  DictionarySource,
  Health,
  Settings,
  Subtitle,
} from "./types";

type Page = "live" | "history" | "settings";
type SubtitleSource = "speaker" | "microphone";
type Lookup = {
  term: string;
  context: string;
  entries: DictionaryEntry[];
  anchor: LookupAnchor;
  range?: Range;
};

const demoParams = new URLSearchParams(window.location.search);
const DEMO_MODE = demoParams.has("demo");
const DEMO_LOOKUP = demoParams.has("lookup");
const DEMO_STOPPED = demoParams.has("stopped");
const DEMO_COMPACT = demoParams.has("compact");
const NATIVE_APP = isTauri();
const CONVERSATION_STARTS_KEY = "vrcs.conversation-starts.v1";
const SIDEBAR_OPEN_KEY = "vrcs.conversation-sidebar-open";

const demoSettings: Settings = {
  host: "127.0.0.1",
  port: 8765,
  database_path: "data/vrcs.db",
  audio_device_id: null,
  microphone_device_id: 2,
  sample_rate: 16000,
  subtitle_history_limit: 500,
  asr: { model: "small", language: "auto", device: "auto", compute_type: "int8" },
};

const demoDevices: AudioDevice[] = [
  { id: 1, name: "Realtek High Definition Audio", is_default: true, is_loopback: true, sample_rate: 48000, channels: 2 },
  { id: 2, name: "默认麦克风", is_default: true, is_loopback: false, sample_rate: 48000, channels: 1 },
  { id: 3, name: "Yeti Stereo Microphone", is_default: false, is_loopback: false, sample_rate: 48000, channels: 2 },
];

const demoSubtitles: Subtitle[] = [
  {
    id: 3,
    text: "このアプリは本当に便利ですね。言語学習にとても役立ちます。",
    language: "ja",
    source: "speaker",
    started_at: null,
    ended_at: null,
    created_at: new Date(Date.now() - 60_000).toISOString(),
  },
  {
    id: 2,
    text: "谢谢你的介绍！这个功能看起来非常实用，对语言学习很有帮助。",
    language: "zh",
    source: "microphone",
    started_at: null,
    ended_at: null,
    created_at: new Date(Date.now() - 120_000).toISOString(),
  },
  {
    id: 1,
    text: "はじめまして、VRCSの世界へようこそ。ここではリアルタイムで翻訳が表示されます。",
    language: "ja",
    source: "speaker",
    started_at: null,
    ended_at: null,
    created_at: new Date(Date.now() - 180_000).toISOString(),
  },
  {
    id: 5,
    text: "昨日はQuest対応のワールドをいくつか巡りました。",
    language: "ja",
    source: "speaker",
    started_at: null,
    ended_at: null,
    created_at: new Date(Date.now() - 3 * 3_600_000).toISOString(),
  },
  {
    id: 4,
    text: "我把不熟悉的表达都记录下来了。",
    language: "zh",
    source: "microphone",
    started_at: null,
    ended_at: null,
    created_at: new Date(Date.now() - 3 * 3_600_000 - 60_000).toISOString(),
  },
  {
    id: 6,
    text: "次回はフレンドと英会話イベントに参加する予定です。",
    language: "ja",
    source: "speaker",
    started_at: null,
    ended_at: null,
    created_at: new Date(Date.now() - 26 * 3_600_000).toISOString(),
  },
];

const demoHealth: Health = {
  status: "ok",
  capture_running: true,
  audio_device: demoDevices[0],
  asr_status: "ready",
  vad_backend: "silero",
  last_error: null,
};

function timestamp(value: string): string {
  return new Intl.DateTimeFormat("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

function storedConversationStarts() {
  try {
    const value = JSON.parse(localStorage.getItem(CONVERSATION_STARTS_KEY) ?? "[]") as unknown;
    return Array.isArray(value) ? value.filter((item): item is number => typeof item === "number" && Number.isFinite(item)).slice(-50) : [];
  } catch {
    return [];
  }
}

function conversationTime(value: string) {
  const date = new Date(value);
  const today = new Date();
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  const sameDay = (left: Date, right: Date) => left.toDateString() === right.toDateString();
  if (sameDay(date, today)) return `今天 ${timestamp(value)}`;
  if (sameDay(date, yesterday)) return `昨天 ${timestamp(value)}`;
  return new Intl.DateTimeFormat("zh-CN", { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" }).format(date);
}

function App() {
  return <MainApp />;
}

function MainApp() {
  const openedAt = useRef(Date.now()).current;
  const [page, setPage] = useState<Page>("live");
  const [connection, setConnection] = useState<ConnectionState>(DEMO_MODE ? "connected" : "connecting");
  const [health, setHealth] = useState<Health | null>(DEMO_MODE ? { ...demoHealth, capture_running: !DEMO_STOPPED } : null);
  const [subtitles, setSubtitles] = useState<Subtitle[]>(DEMO_MODE ? demoSubtitles : []);
  const [settings, setSettings] = useState<Settings | null>(DEMO_MODE ? demoSettings : null);
  const [devices, setDevices] = useState<AudioDevice[]>(DEMO_MODE ? demoDevices : []);
  const [dictionarySources, setDictionarySources] = useState<DictionarySource[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [lookup, setLookup] = useState<Lookup | null>(DEMO_MODE && DEMO_LOOKUP ? {
    term: "便利",
    context: demoSubtitles[0].text,
    entries: [{ term: "便利", reading: "べんり", language: "ja", definition: "方便的；有用的；省事的" }],
    anchor: { top: 386, bottom: 408, centerX: 432 },
  } : null);
  const [compact, setCompact] = useState(DEMO_COMPACT);
  const [sidebarOpen, setSidebarOpen] = useState(() => localStorage.getItem(SIDEBAR_OPEN_KEY) !== "false");
  const [conversationStarts, setConversationStarts] = useState(storedConversationStarts);
  const [selectedConversationId, setSelectedConversationId] = useState<string | null>(null);
  const conversations = useMemo(
    () => groupConversations(subtitles, conversationStarts, openedAt),
    [conversationStarts, openedAt, subtitles],
  );
  const activeConversation = conversations[0];
  const selectedConversation = conversations.find((conversation) => conversation.id === selectedConversationId) ?? activeConversation;

  useEffect(() => {
    localStorage.setItem(SIDEBAR_OPEN_KEY, String(sidebarOpen));
  }, [sidebarOpen]);

  useEffect(() => {
    localStorage.setItem(CONVERSATION_STARTS_KEY, JSON.stringify(conversationStarts));
  }, [conversationStarts]);

  useEffect(() => {
    if (activeConversation && !conversations.some((conversation) => conversation.id === selectedConversationId)) {
      setSelectedConversationId(activeConversation.id);
    }
  }, [activeConversation, conversations, selectedConversationId]);

  const refresh = useCallback(async () => {
    if (DEMO_MODE) return;
    try {
      const [nextHealth, nextSettings, historyItems] = await Promise.all([
        coreApi.health(),
        coreApi.settings(),
        coreApi.subtitles(),
      ]);
      setHealth(nextHealth);
      setSettings(nextSettings);
      setSubtitles(historyItems);
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "无法连接 Core 服务");
    }
  }, []);

  useEffect(() => {
    if (DEMO_MODE) return;
    void refresh();
    const timer = window.setInterval(
      () => void coreApi.health().then(setHealth).catch(() => setHealth(null)),
      2500,
    );
    return () => window.clearInterval(timer);
  }, [refresh]);

  useEffect(() => {
    if (DEMO_MODE) return;
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

  const loadDevices = useCallback(async () => {
    if (DEMO_MODE) return;
    try {
      setDevices(await coreApi.devices());
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "设备枚举失败");
    }
  }, []);

  const loadDictionaries = useCallback(async () => {
    if (DEMO_MODE) return;
    try {
      setDictionarySources(await coreApi.dictionaries());
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "词典列表加载失败");
    }
  }, []);

  useEffect(() => {
    if (page === "settings") void Promise.all([loadDevices(), loadDictionaries()]);
  }, [loadDevices, loadDictionaries, page]);

  const toggleCapture = async () => {
    if (DEMO_MODE) {
      setHealth((current) => current ? { ...current, capture_running: !current.capture_running } : current);
      return;
    }
    try {
      if (health?.capture_running) await coreApi.stop();
      else await coreApi.start(
        settings?.audio_device_id ?? null,
        settings?.microphone_device_id ?? null,
      );
      setHealth(await coreApi.health());
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "操作失败");
    }
  };

  const saveSettings = async (next: Settings) => {
    if (DEMO_MODE) {
      setSettings(next);
      return;
    }
    try {
      setSettings(await coreApi.saveSettings(next));
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "设置保存失败");
    }
  };

  const importDictionary = async (file: File) => {
    if (DEMO_MODE) {
      const imported: DictionarySource = {
        id: Date.now(),
        title: file.name.replace(/\.zip$/i, ""),
        revision: "demo",
        source_language: "ja",
        target_language: "zh",
        entry_count: 128_430,
        imported_at: new Date().toISOString(),
      };
      setDictionarySources((current) => [imported, ...current.filter((item) => item.title !== imported.title)]);
      return imported;
    }
    const imported = await coreApi.importDictionary(file);
    await loadDictionaries();
    return imported;
  };

  const deleteDictionary = async (id: number) => {
    if (DEMO_MODE) {
      setDictionarySources((current) => current.filter((item) => item.id !== id));
      return;
    }
    await coreApi.deleteDictionary(id);
    await loadDictionaries();
  };

  const selectWord = async (context: string) => {
    const selection = window.getSelection();
    const term = selection?.toString().trim().replace(
      /^[\s.,!?;:，。！？；：“”'"「」『』（）()]+|[\s.,!?;:，。！？；：“”'"「」『』（）()]+$/g,
      "",
    );
    if (!selection || !term || selection.rangeCount === 0) return;
    const range = selection.getRangeAt(0).cloneRange();
    const rect = range.getBoundingClientRect();
    if (!rect.width && !rect.height) return;
    try {
      const entries = DEMO_MODE
        ? [{ term, reading: term === "便利" ? "べんり" : "", language: "ja", definition: "方便的；有用的；省事的" }]
        : await coreApi.lookup(term);
      const nextLookup: Lookup = {
        term,
        context,
        entries,
        anchor: { top: rect.top, bottom: rect.bottom, centerX: rect.left + rect.width / 2 },
        range,
      };
      setLookup(nextLookup);
      if (compact) await resizeCompactWindow(true);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "查词失败");
    }
  };

  const resizeCompactWindow = async (lookupOpen: boolean) => {
    if (!NATIVE_APP) return;
    const { getCurrentWindow, LogicalSize } = await import("@tauri-apps/api/window");
    const size = compactWindowSize(lookupOpen);
    await getCurrentWindow().setSize(new LogicalSize(size.width, size.height));
  };

  const closeCompactLookup = () => {
    setLookup(null);
    void resizeCompactWindow(false).catch((reason) => {
      setError(reason instanceof Error ? reason.message : "小窗收起失败");
    });
  };

  const toggleCompact = async () => {
    const next = !compact;
    try {
      if (!NATIVE_APP) {
        if (!next) setLookup(null);
        setCompact(next);
        return;
      }

      const { getCurrentWindow, LogicalSize } = await import("@tauri-apps/api/window");
      const appWindow = getCurrentWindow();
      if (next) {
        const compactSize = new LogicalSize(COMPACT_WINDOW_SIZE.width, COMPACT_WINDOW_SIZE.height);
        await appWindow.setMinSize(compactSize);
        await appWindow.setSize(compactSize);
        await appWindow.setResizable(false);
        await appWindow.setAlwaysOnTop(true);
      } else {
        setLookup(null);
        await appWindow.setAlwaysOnTop(false);
        await appWindow.setResizable(true);
        await appWindow.setMinSize(new LogicalSize(860, 620));
        await appWindow.setSize(new LogicalSize(1180, 760));
      }

      if (await appWindow.isAlwaysOnTop() !== next) {
        throw new Error("窗口置顶状态未生效");
      }
      setCompact(next);
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "小窗模式切换失败");
    }
  };

  const closeWindow = async () => {
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().close();
    } catch {
      setCompact(false);
    }
  };

  const createConversation = () => {
    if (activeConversation && !activeConversation.subtitles.length) {
      setSelectedConversationId(activeConversation.id);
      return;
    }
    const latestSubtitleAt = subtitles.reduce(
      (latest, subtitle) => Math.max(latest, Date.parse(subtitle.created_at) || 0),
      0,
    );
    const latestBoundary = conversationStarts[conversationStarts.length - 1] ?? 0;
    const startedAt = Math.max(Date.now(), latestSubtitleAt + 1, latestBoundary + 1);
    setConversationStarts((current) => [...current, startedAt].sort((left, right) => left - right).slice(-50));
    setSelectedConversationId(conversationId(startedAt));
    setLookup(null);
  };

  if (compact) {
    const compactSubtitle = subtitleForCompactView(subtitles, lookup?.context);
    return (
      <div className={`compact-root ${lookup ? "compact-root-lookup" : ""}`}>
        <CompactView
          subtitle={compactSubtitle}
          running={health?.capture_running ?? false}
          onSelect={selectWord}
          onCapture={() => void toggleCapture()}
          onRestore={() => void toggleCompact()}
          onClose={() => void closeWindow()}
        />
        {lookup && <DictionaryPopover lookup={lookup} demo={DEMO_MODE} compact onClose={closeCompactLookup} />}
      </div>
    );
  }

  return (
    <div className={`app-shell ${page === "live" ? "live-shell" : ""} ${sidebarOpen ? "sidebar-open" : "sidebar-collapsed"}`}>
      <WindowChrome />
      <div className="app-body">
        {page === "live" && (
          <ConversationSidebar
            open={sidebarOpen}
            conversations={conversations}
            activeId={activeConversation?.id}
            selectedId={selectedConversation?.id}
            onToggle={() => setSidebarOpen((current) => !current)}
            onNew={createConversation}
            onSelect={(id) => { setSelectedConversationId(id); setLookup(null); }}
          />
        )}
        {page === "live" && sidebarOpen && <button className="sidebar-scrim" type="button" aria-label="关闭对话侧栏" onClick={() => setSidebarOpen(false)} />}
        <div className="app-scroll-region">
          <main className={`workspace workspace-${page}`}>
          {page === "live" && <TopStatus connection={connection} health={health} settings={settings} />}

          {error && (
            <div className="error-banner" role="alert">
              <span>{error}</span>
              <button type="button" aria-label="关闭错误提示" onClick={() => setError(null)}><X size={18} /></button>
            </div>
          )}

          {page === "live" && (
            <>
              {selectedConversation && activeConversation && selectedConversation.id !== activeConversation.id && (
                <div className="conversation-history-notice">
                  <Clock3 size={15} />
                  <span>正在查看 {conversationTime(selectedConversation.startedAt)} 的对话</span>
                  <button type="button" onClick={() => setSelectedConversationId(activeConversation.id)}>返回当前</button>
                </div>
              )}
              <LiveView
                subtitles={(selectedConversation?.subtitles ?? []).slice(0, 12)}
                running={(health?.capture_running ?? false) && selectedConversation?.id === activeConversation?.id}
                onSelect={selectWord}
              />
            </>
          )}

          {page === "history" && (
            <HistoryView subtitles={subtitles} onSelect={selectWord} />
          )}

          {page === "settings" && settings && (
            <SettingsPanel
              settings={settings}
              devices={devices}
              dictionaries={dictionarySources}
              disabled={health?.capture_running ?? false}
              modelStatus={health?.asr_status ?? "unknown"}
              onRefresh={loadDevices}
              onImportDictionary={importDictionary}
              onDeleteDictionary={deleteDictionary}
              onSave={saveSettings}
            />
          )}
          </main>
        </div>
      </div>

      <BottomDock
        page={page}
        running={health?.capture_running ?? false}
        onPageChange={(next) => { setLookup(null); setPage(next); }}
        onCompact={() => void toggleCompact()}
        onCapture={() => void toggleCapture()}
      />

      {lookup && <DictionaryPopover lookup={lookup} demo={DEMO_MODE} onClose={() => setLookup(null)} />}
    </div>
  );
}

function ConversationSidebar({ open, conversations, activeId, selectedId, onToggle, onNew, onSelect }: {
  open: boolean;
  conversations: SubtitleConversation[];
  activeId?: string;
  selectedId?: string;
  onToggle: () => void;
  onNew: () => void;
  onSelect: (id: string) => void;
}) {
  const active = conversations.find((conversation) => conversation.id === activeId);
  const history = conversations.filter((conversation) => conversation.id !== activeId);

  if (!open) {
    return (
      <aside className="conversation-sidebar conversation-sidebar-collapsed" aria-label="对话侧栏">
        <button className="sidebar-icon-button" type="button" aria-label="展开对话侧栏" aria-expanded="false" onClick={onToggle}><PanelLeftOpen size={19} /></button>
        <button className="sidebar-icon-button sidebar-new-icon" type="button" aria-label="新建对话" onClick={onNew}><Plus size={20} /></button>
        {active && <button className={`sidebar-icon-button sidebar-current-icon ${selectedId === active.id ? "active" : ""}`} type="button" aria-label="查看当前对话" onClick={() => onSelect(active.id)}><MessageSquareText size={19} /></button>}
      </aside>
    );
  }

  return (
    <aside className="conversation-sidebar" aria-label="对话侧栏">
      <div className="conversation-sidebar-header">
        <span>对话</span>
        <button className="sidebar-icon-button" type="button" aria-label="收起对话侧栏" aria-expanded="true" onClick={onToggle}><PanelLeftClose size={19} /></button>
      </div>
      <button className="new-conversation-button" type="button" onClick={onNew}><Plus size={18} />新建对话</button>
      <div className="conversation-sidebar-list">
        {active && (
          <section className="conversation-group" aria-labelledby="current-conversation-heading">
            <h2 id="current-conversation-heading">当前对话</h2>
            <ConversationButton conversation={active} active selected={selectedId === active.id} onSelect={onSelect} />
          </section>
        )}
        <section className="conversation-group" aria-labelledby="recent-conversations-heading">
          <h2 id="recent-conversations-heading">以往对话</h2>
          {history.length ? history.map((conversation) => (
            <ConversationButton key={conversation.id} conversation={conversation} selected={selectedId === conversation.id} onSelect={onSelect} />
          )) : <p className="conversation-list-empty">历史对话会显示在这里</p>}
        </section>
      </div>
    </aside>
  );
}

function ConversationButton({ conversation, active = false, selected, onSelect }: {
  conversation: SubtitleConversation;
  active?: boolean;
  selected: boolean;
  onSelect: (id: string) => void;
}) {
  return (
    <button
      className={`conversation-button ${selected ? "selected" : ""}`}
      type="button"
      aria-current={selected ? "true" : undefined}
      onClick={() => onSelect(conversation.id)}
    >
      <span className="conversation-button-title"><MessageSquareText size={16} /><strong>{conversation.title}</strong>{active && <i aria-label="当前" />}</span>
      <span className="conversation-button-meta"><time>{conversationTime(conversation.startedAt)}</time><span>{conversation.subtitles.length} 条字幕</span></span>
    </button>
  );
}

function WindowChrome() {
  const runWindowAction = async (action: "minimize" | "maximize" | "close") => {
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      const appWindow = getCurrentWindow();
      if (action === "minimize") await appWindow.minimize();
      if (action === "maximize") await appWindow.toggleMaximize();
      if (action === "close") await appWindow.close();
    } catch {
      // Window controls are intentionally inactive in the browser preview.
    }
  };

  return (
    <header className="window-chrome" data-tauri-drag-region aria-label="窗口控制区">
      <div className="window-drag-region" data-tauri-drag-region />
      <div className="window-actions">
        <button type="button" aria-label="最小化窗口" title="最小化" onClick={() => void runWindowAction("minimize")}><Minus size={15} strokeWidth={1.8} /></button>
        <button type="button" aria-label="最大化或还原窗口" title="最大化或还原" onClick={() => void runWindowAction("maximize")}><Square size={12} strokeWidth={1.7} /></button>
        <button className="window-close" type="button" aria-label="关闭窗口" title="关闭" onClick={() => void runWindowAction("close")}><X size={15} strokeWidth={1.8} /></button>
      </div>
    </header>
  );
}

function TopStatus({ connection, health, settings }: {
  connection: ConnectionState;
  health: Health | null;
  settings: Settings | null;
}) {
  const connectionLabel = connection === "connected" ? "已连接" : connection === "connecting" ? "连接中" : "未连接";
  return (
    <div className="top-status-row">
      <div className="status-summary" aria-label="连接与转写状态">
        <div className={`core-summary connection-${connection}`}><span>Core</span><strong><i aria-hidden="true" />{connectionLabel}</strong></div>
        <i aria-hidden="true" />
        <div><span>状态</span><strong>{health?.capture_running ? "正在转写" : "等待开始"}</strong></div>
        <i aria-hidden="true" />
        <div><span>引擎</span><strong>Whisper {capitalize(settings?.asr.model ?? "small")}</strong></div>
      </div>
    </div>
  );
}

function capitalize(value: string): string {
  return value.charAt(0).toUpperCase() + value.slice(1);
}

function contextExcerpt(context: string, term: string): string {
  return context.split(/(?<=[。！？.!?])/).find((sentence) => sentence.includes(term))?.trim() ?? context;
}

function LiveView({ subtitles, running, onSelect }: {
  subtitles: Subtitle[];
  running: boolean;
  onSelect: (context: string) => Promise<void>;
}) {
  const chronological = [...subtitles].reverse();
  return (
    <section className="conversation" aria-label="实时字幕">
      {chronological.length ? chronological.map((subtitle, index) => (
        <ChatBubble key={subtitle.id ?? `${subtitle.created_at}-${index}`} subtitle={subtitle} onSelect={onSelect} />
      )) : (
        <div className="empty-state"><MessageSquare size={22} /><p>{running ? "正在聆听，新的字幕会出现在这里。" : "开始转写后，字幕会显示在这里。"}</p></div>
      )}
      {running && (
        <div className="message-group source-speaker streaming-message">
          <div className="bubble">转写中<span className="streaming-ellipsis" aria-hidden="true">…</span></div>
        </div>
      )}
    </section>
  );
}

function ChatBubble({ subtitle, onSelect }: { subtitle: Subtitle; onSelect: (context: string) => Promise<void> }) {
  const source: SubtitleSource = subtitle.source ?? "speaker";
  const mine = source === "microphone";
  return (
    <article className={`message-group source-${source}`}>
      <div className="message-meta">
        {!mine && <Volume2 size={14} />}
        {mine && <time>{timestamp(subtitle.created_at)}</time>}
        <span>{mine ? "麦克风 · 我" : "扬声器 · 对方"}</span>
        {!mine && <time>{timestamp(subtitle.created_at)}</time>}
        {mine && <Mic size={14} />}
      </div>
      <p className="bubble" onMouseUp={() => void onSelect(subtitle.text)}>{subtitle.text}</p>
    </article>
  );
}

function HistoryView({ subtitles, onSelect }: { subtitles: Subtitle[]; onSelect: (context: string) => Promise<void> }) {
  const [language, setLanguage] = useState("all");
  const [range, setRange] = useState("all");
  const filtered = useMemo(() => {
    const now = Date.now();
    return subtitles.filter((subtitle) => {
      if (language !== "all" && subtitle.language !== language) return false;
      if (range === "today" && now - new Date(subtitle.created_at).getTime() > 86_400_000) return false;
      if (range === "week" && now - new Date(subtitle.created_at).getTime() > 604_800_000) return false;
      return true;
    });
  }, [language, range, subtitles]);

  return (
    <section className="history-surface">
      <div className="history-toolbar">
        <div><h2>字幕历史</h2><span>共 {filtered.length} 条记录</span></div>
        <div className="history-filters">
          <DropdownField
            compact
            icon={<Languages size={15} />}
            label="语言"
            value={language}
            options={[
              { value: "all", label: "全部语言" },
              { value: "ja", label: "日语" },
              { value: "en", label: "英语" },
              { value: "zh", label: "中文" },
              { value: "ko", label: "韩语" },
            ]}
            onChange={setLanguage}
          />
          <DropdownField
            compact
            icon={<CalendarDays size={15} />}
            label="日期范围"
            value={range}
            options={[
              { value: "all", label: "全部时间" },
              { value: "today", label: "今天" },
              { value: "week", label: "最近 7 天" },
            ]}
            onChange={setRange}
          />
        </div>
      </div>
      {filtered.length ? (
        <div className="history-list">{filtered.map((subtitle, index) => (
          <article key={subtitle.id ?? `${subtitle.created_at}-${index}`} onMouseUp={() => void onSelect(subtitle.text)}>
            <time>{timestamp(subtitle.created_at)}</time>
            <p>{subtitle.text}</p>
            <span>{subtitle.language?.toUpperCase() ?? "—"}</span>
          </article>
        ))}</div>
      ) : <div className="empty-state"><History size={22} /><p>没有符合筛选条件的字幕。</p></div>}
    </section>
  );
}

function SettingsPanel({ settings, devices, dictionaries, disabled, modelStatus, onRefresh, onImportDictionary, onDeleteDictionary, onSave }: {
  settings: Settings;
  devices: AudioDevice[];
  dictionaries: DictionarySource[];
  disabled: boolean;
  modelStatus: string;
  onRefresh: () => Promise<void>;
  onImportDictionary: (file: File) => Promise<DictionarySource>;
  onDeleteDictionary: (id: number) => Promise<void>;
  onSave: (value: Settings) => Promise<void>;
}) {
  const [draft, setDraft] = useState(settings);
  const [saved, setSaved] = useState(false);
  const [dictionaryBusy, setDictionaryBusy] = useState(false);
  const [dictionaryMessage, setDictionaryMessage] = useState("");
  const dictionaryFileRef = useRef<HTMLInputElement>(null);
  useEffect(() => setDraft(settings), [settings]);
  const updateAsr = (key: keyof Settings["asr"], value: string) => {
    setDraft({ ...draft, asr: { ...draft.asr, [key]: value } });
    setSaved(false);
  };
  const outputDevices = devices.filter((device) => device.is_loopback);
  const microphoneDevices = devices.filter((device) => !device.is_loopback);
  const save = async () => {
    await onSave(draft);
    setSaved(true);
  };
  const chooseDictionary = async (file?: File) => {
    if (!file) return;
    setDictionaryBusy(true);
    setDictionaryMessage(`正在导入 ${file.name}…`);
    try {
      const imported = await onImportDictionary(file);
      setDictionaryMessage(`已导入 ${imported.title}，共 ${imported.entry_count.toLocaleString("zh-CN")} 条词条`);
    } catch (reason) {
      setDictionaryMessage(reason instanceof Error ? reason.message : "词典导入失败");
    } finally {
      setDictionaryBusy(false);
      if (dictionaryFileRef.current) dictionaryFileRef.current.value = "";
    }
  };
  const removeDictionary = async (dictionary: DictionarySource) => {
    if (!window.confirm(`确定移除词典“${dictionary.title}”吗？`)) return;
    setDictionaryBusy(true);
    try {
      await onDeleteDictionary(dictionary.id);
      setDictionaryMessage(`已移除 ${dictionary.title}`);
    } catch (reason) {
      setDictionaryMessage(reason instanceof Error ? reason.message : "词典移除失败");
    } finally {
      setDictionaryBusy(false);
    }
  };

  return (
    <section className="settings-surface">
      <div className="settings-section">
        <div className="section-heading">
          <div><h2>识别引擎</h2><span className="status-chip">状态：{modelStatus}</span></div>
          <p>{disabled ? "停止转写后可修改" : "本地 Whisper 配置"}</p>
        </div>
        <div className="form-grid">
          <Select label="模型" helper="平衡速度与精度，适合大多数场景" value={draft.asr.model} values={["tiny", "base", "small", "medium", "large-v3"]} disabled={disabled} onChange={(value) => updateAsr("model", value)} />
          <Select label="语言" helper="保留原语言进行转写，不进行翻译" value={draft.asr.language} values={["auto", "en", "ja", "zh", "ko", "es", "fr", "de"]} disabled={disabled} onChange={(value) => updateAsr("language", value)} />
          <Select label="运行设备" value={draft.asr.device} values={["auto", "cpu", "cuda"]} disabled={disabled} onChange={(value) => updateAsr("device", value)} />
          <Select label="计算类型" value={draft.asr.compute_type} values={["int8", "float16", "int8_float16"]} disabled={disabled} onChange={(value) => updateAsr("compute_type", value)} />
        </div>
      </div>

      <div className="settings-divider" />

      <div className="settings-section audio-section">
        <div className="section-heading">
          <div><h2>音频来源</h2><span>{devices.length ? `已发现 ${devices.length} 个设备` : "等待扫描"}</span></div>
          <button className="secondary-button" type="button" onClick={() => void onRefresh()}><RefreshCw size={15} />重新扫描</button>
        </div>

        <DeviceGroup
          icon={<Volume2 size={18} />}
          title="系统音频输出 · 对方"
          devices={outputDevices}
          selected={draft.audio_device_id}
          includeDefault
          disabled={disabled}
          onSelect={(id) => { setDraft({ ...draft, audio_device_id: id }); setSaved(false); }}
        />
        <DeviceGroup
          icon={<Mic size={18} />}
          title="麦克风输入 · 我"
          note="启用后将识别为自己的发言"
          devices={microphoneDevices}
          selected={draft.microphone_device_id}
          offLabel="关闭麦克风"
          disabled={disabled}
          onSelect={(id) => { setDraft({ ...draft, microphone_device_id: id }); setSaved(false); }}
        />
      </div>

      <div className="settings-divider" />

      <div className="settings-section dictionary-section">
        <div className="section-heading">
          <div><BookOpen size={18} /><h2>Yomitan 词典</h2><span>{dictionaries.length ? `已导入 ${dictionaries.length} 部` : "尚未导入"}</span></div>
          <button className="secondary-button" type="button" disabled={dictionaryBusy} onClick={() => dictionaryFileRef.current?.click()}><Upload size={15} />导入词典</button>
          <input
            ref={dictionaryFileRef}
            className="dictionary-file-input"
            type="file"
            accept=".zip,application/zip"
            onChange={(event) => void chooseDictionary(event.target.files?.[0])}
          />
        </div>
        {dictionaries.length ? (
          <div className="dictionary-source-list">
            {dictionaries.map((dictionary) => (
              <div className="dictionary-source-row" key={dictionary.id}>
                <div className="dictionary-source-icon"><BookOpen size={17} /></div>
                <div>
                  <strong>{dictionary.title}</strong>
                  <span>{dictionary.source_language.toUpperCase()}{dictionary.target_language ? ` → ${dictionary.target_language.toUpperCase()}` : ""} · {dictionary.entry_count.toLocaleString("zh-CN")} 条 · {dictionary.revision}</span>
                </div>
                <button type="button" disabled={dictionaryBusy} aria-label={`移除 ${dictionary.title}`} title="移除词典" onClick={() => void removeDictionary(dictionary)}><Trash2 size={16} /></button>
              </div>
            ))}
          </div>
        ) : <p className="dictionary-empty">导入 Yomitan ZIP 词典后，划词查询会优先显示其中的释义。</p>}
        {dictionaryMessage && <p className="dictionary-feedback" role="status">{dictionaryMessage}</p>}
      </div>

      <div className="settings-actions">
        <span>{saved ? "设置已保存" : "所有更改将在下次转写时生效"}</span>
        <button className="primary-button" type="button" disabled={disabled} onClick={() => void save()}>保存设置</button>
      </div>
    </section>
  );
}

function Select({ label, helper, value, values, disabled, onChange }: {
  label: string;
  helper?: string;
  value: string;
  values: string[];
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <div className="field">
      <span>{label}</span>
      <DropdownField
        label={label}
        value={value}
        options={values.map((item) => ({ value: item, label: item }))}
        disabled={disabled}
        onChange={onChange}
      />
      {helper && <small>{helper}</small>}
    </div>
  );
}

function DropdownField({ label, value, options, disabled = false, compact = false, icon, onChange }: {
  label: string;
  value: string;
  options: Array<{ value: string; label: string }>;
  disabled?: boolean;
  compact?: boolean;
  icon?: React.ReactNode;
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const selected = options.find((option) => option.value === value) ?? options[0];

  useEffect(() => {
    if (!open) return;
    const closeOnOutside = (event: PointerEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) setOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", closeOnOutside);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutside);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [open]);

  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);

  const choose = (next: string) => {
    onChange(next);
    setOpen(false);
  };

  return (
    <div className={`dropdown-field ${compact ? "dropdown-field-compact" : ""} ${open ? "open" : ""}`} ref={rootRef}>
      <button
        className="dropdown-trigger"
        type="button"
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={compact ? label : undefined}
        onClick={() => setOpen((current) => !current)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            setOpen(true);
          }
        }}
      >
        {icon && <span className="dropdown-icon">{icon}</span>}
        <span className="dropdown-value">{selected?.label ?? value}</span>
        <ChevronDown className="dropdown-chevron" size={16} />
      </button>
      {open && (
        <div className="dropdown-menu" role="listbox" aria-label={label}>
          {options.map((option) => {
            const current = option.value === value;
            return (
              <button
                className={`dropdown-option ${current ? "selected" : ""}`}
                key={option.value}
                type="button"
                role="option"
                aria-selected={current}
                onClick={() => choose(option.value)}
              >
                <span>{option.label}</span>
                {current && <Check size={15} />}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}

function DeviceGroup({ icon, title, note, devices, selected, includeDefault = false, offLabel, disabled, onSelect }: {
  icon: React.ReactNode;
  title: string;
  note?: string;
  devices: AudioDevice[];
  selected: number | null;
  includeDefault?: boolean;
  offLabel?: string;
  disabled: boolean;
  onSelect: (id: number | null) => void;
}) {
  const defaultDevice = devices.find((device) => device.is_default);
  const defaultDescription = defaultDevice
    ? `跟随 Windows 默认设备 · ${defaultDevice.sample_rate} Hz · ${defaultDevice.channels} 声道`
    : "跟随 Windows 默认设备";
  return (
    <div className="device-group">
      <div className="device-group-title">{icon}<div><h3>{title}</h3>{note && <span>{note}</span>}</div></div>
      <div className="device-list">
        {includeDefault && <DeviceRow name="系统默认输出" description={defaultDescription} chosen={selected === null} disabled={disabled} onSelect={() => onSelect(null)} />}
        {offLabel && <DeviceRow name={offLabel} description="仅转写系统音频" chosen={selected === null} disabled={disabled} onSelect={() => onSelect(null)} />}
        {devices.map((device) => (
          <DeviceRow
            key={device.id}
            name={device.name}
            description={`${device.sample_rate} Hz · ${device.channels} 声道${device.is_default ? " · 默认" : ""}`}
            chosen={selected === device.id}
            disabled={disabled}
            onSelect={() => onSelect(device.id)}
          />
        ))}
        {!devices.length && !includeDefault && !offLabel && <p className="device-empty">未发现可用设备。</p>}
      </div>
    </div>
  );
}

function DeviceRow({ name, description, chosen, disabled, onSelect }: {
  name: string;
  description: string;
  chosen: boolean;
  disabled: boolean;
  onSelect: () => void;
}) {
  return (
    <label className={`device-row ${chosen ? "chosen" : ""}`}>
      <input type="radio" checked={chosen} disabled={disabled} onChange={onSelect} />
      <span><strong>{name}</strong><small>{description}</small></span>
    </label>
  );
}

function BottomDock({ page, running, onPageChange, onCompact, onCapture }: {
  page: Page;
  running: boolean;
  onPageChange: (page: Page) => void;
  onCompact: () => void;
  onCapture: () => void;
}) {
  return (
    <nav className="bottom-dock" aria-label="主导航">
      <DockButton label="实时字幕" active={page === "live"} onClick={() => onPageChange("live")}><MessageSquare /></DockButton>
      <DockButton label="字幕历史" active={page === "history"} onClick={() => onPageChange("history")}><History /></DockButton>
      <DockButton label="设置" active={page === "settings"} onClick={() => onPageChange("settings")}><SlidersHorizontal /></DockButton>
      <i className="dock-divider" aria-hidden="true" />
      <DockButton label="字幕模式" tonal onClick={onCompact}><Shrink /></DockButton>
      <DockButton label={running ? "停止转写" : "开始转写"} primary onClick={onCapture}>{running ? <Square /> : <Mic />}</DockButton>
    </nav>
  );
}

function DockButton({ label, active = false, tonal = false, primary = false, onClick, children }: {
  label: string;
  active?: boolean;
  tonal?: boolean;
  primary?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      className={`dock-button ${active ? "active" : ""} ${tonal ? "tonal" : ""} ${primary ? "primary" : ""}`}
      aria-label={label}
      aria-current={active ? "page" : undefined}
      data-tooltip={label}
      title={label}
      onClick={onClick}
    >{children}</button>
  );
}

function DictionaryPopover({ lookup, demo, compact = false, onClose }: { lookup: Lookup; demo: boolean; compact?: boolean; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [message, setMessage] = useState("");
  const [anchor, setAnchor] = useState(lookup.anchor);
  const groupedEntries = groupDictionaryEntries(lookup.entries);
  const entry = groupedEntries[0];
  const visibleEntries = groupedEntries.slice(0, 6);
  const width = Math.min(340, window.innerWidth - 24);
  const placement = placeLookupPopover({
    anchor,
    popoverHeight: LOOKUP_POPOVER_HEIGHT,
    viewportHeight: window.innerHeight,
    viewportTop: 40,
  });
  const left = Math.min(Math.max(12, anchor.centerX - 34), window.innerWidth - width - 12);
  const arrowLeft = Math.min(Math.max(22, anchor.centerX - left - 8), width - 38);
  const style = compact
    ? undefined
    : { left, top: placement.top, width, height: placement.height, "--arrow-left": `${arrowLeft}px` };

  useEffect(() => {
    if (compact || !lookup.range) return;

    const updateAnchor = () => {
      const rect = lookup.range?.getBoundingClientRect();
      if (!rect || !isLookupAnchorVisible(
        rect,
        window.innerWidth,
        window.innerHeight,
        40,
      )) {
        onClose();
        return;
      }
      setAnchor({ top: rect.top, bottom: rect.bottom, centerX: rect.left + rect.width / 2 });
    };

    updateAnchor();
    window.addEventListener("scroll", updateAnchor, true);
    window.addEventListener("resize", updateAnchor);
    return () => {
      window.removeEventListener("scroll", updateAnchor, true);
      window.removeEventListener("resize", updateAnchor);
    };
  }, [compact, lookup.range, onClose]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => { if (event.key === "Escape") onClose(); };
    const onPointerDown = (event: PointerEvent) => {
      if (!compact && ref.current && !ref.current.contains(event.target as Node)) onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [compact, onClose]);

  const add = async () => {
    if (!entry) return;
    if (demo) {
      setMessage("已添加到 Anki");
      return;
    }
    try {
      const result = await coreApi.createCard(lookup.term, entry.definition, lookup.context);
      setMessage(`已创建卡片 #${result.note_id}`);
    } catch (reason) {
      setMessage(reason instanceof Error ? reason.message : "制卡失败");
    }
  };

  return (
    <div ref={ref} className={`dictionary-popover ${compact ? "compact-inline-dictionary" : `popover-${placement.side}`}`} style={style as CSSProperties} role="dialog" aria-label={`${lookup.term} 的词典解释`}>
      <div className="dictionary-header">
        <div><h2>{lookup.term}</h2>{entry?.reading && <span className="reading">{entry.reading}</span>}{entry && <span className="language-chip">{entry.language.toUpperCase()}</span>}</div>
        <button type="button" aria-label="关闭词典" onClick={onClose}><X size={19} /></button>
      </div>
      <div className="dictionary-scroll">
        {visibleEntries.length ? (
          <div className="dictionary-definitions">
            {visibleEntries.map((item, index) => (
              <article className="dictionary-definition-item" key={`${item.dictionary ?? "local"}-${item.term}-${item.reading ?? ""}-${index}`}>
                <div className="dictionary-entry-meta">
                  <span className="dictionary-source-name">{item.dictionary || "内置词典"}</span>
                  {visibleEntries.length > 1 && <span className="dictionary-entry-index">{String(index + 1).padStart(2, "0")}</span>}
                </div>
                <ol className="definition-glosses">
                  {definitionGlosses(item.definition).map((gloss, glossIndex) => <li key={`${gloss}-${glossIndex}`}>{gloss}</li>)}
                </ol>
              </article>
            ))}
          </div>
        ) : <p className="definition muted">已导入的词典中暂无释义。</p>}
        <div className="lookup-context"><span>原文语境</span><q>{contextExcerpt(lookup.context, lookup.term)}</q></div>
      </div>
      <button className="anki-button" type="button" disabled={!entry} onClick={() => void add()}><PlusCircle size={16} />{message || "添加到 Anki"}</button>
      {!compact && <i className="popover-arrow" aria-hidden="true" />}
    </div>
  );
}

function CompactView({ subtitle, running, onSelect, onCapture, onRestore, onClose }: {
  subtitle?: Subtitle;
  running: boolean;
  onSelect: (context: string) => Promise<void>;
  onCapture: () => void;
  onRestore: () => void;
  onClose: () => void;
}) {
  return (
    <div className="compact-shell">
      <div className="compact-drag-region" data-tauri-drag-region />
      <div className="compact-status"><i className={running ? "running" : ""} />{subtitle?.language?.toUpperCase() ?? "AUTO"}</div>
      <p onMouseUp={() => subtitle && void onSelect(subtitle.text)}>{subtitle?.text ?? "等待字幕…"}</p>
      <div className="compact-actions">
        <button className={`compact-capture-button ${running ? "running" : ""}`} type="button" aria-label={running ? "暂停转录" : "开始转录"} title={running ? "暂停转录" : "开始转录"} onClick={onCapture}>
          {running ? <Square size={15} /> : <Mic size={16} />}
        </button>
        <button className="compact-secondary-action" type="button" aria-label="恢复完整窗口" title="恢复完整窗口" onClick={onRestore}><MessageSquare size={17} /></button>
        <button className="compact-secondary-action" type="button" aria-label="关闭窗口" title="关闭窗口" onClick={onClose}><X size={17} /></button>
      </div>
    </div>
  );
}

export default App;
