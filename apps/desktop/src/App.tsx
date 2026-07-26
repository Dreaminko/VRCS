import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties } from "react";
import {
  CalendarDays,
  BookOpen,
  Check,
  ChevronDown,
  ChevronRight,
  Clock3,
  History,
  HardDrive,
  Download,
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
  TriangleAlert,
  Upload,
  Volume2,
  Wrench,
  X,
} from "lucide-react";
import { coreApi, coreWebSocketUrl, initializeCoreApi } from "./api";
import { ankiButtonLabel } from "./anki";
import type { AnkiAddState } from "./anki";
import {
  ankiDeckAncestors,
  ankiDeckDisplayName,
  ankiDeckParent,
  buildAnkiDeckTree,
  visibleAnkiDeckNodes,
} from "./anki-decks";
import { isTauri } from "@tauri-apps/api/core";
import {
  COMPACT_WINDOW_SIZE,
  compactWindowSize,
  subtitleForCompactView,
} from "./compact-mode";
import { conversationId, groupConversations } from "./conversations";
import type { SubtitleConversation } from "./conversations";
import { shouldShowVrchatNotRunningWarning } from "./capture-warning";
import {
  defaultDesktopPreferences,
  loadDesktopPreferences,
  updateDesktopPreference,
} from "./desktop-preferences";
import type { DesktopPreferences } from "./desktop-preferences";
import { ankiDictionaryContent, definitionGlosses, groupDictionaryEntries } from "./dictionary";
import { isLookupAnchorVisible, LOOKUP_POPOVER_HEIGHT, placeLookupPopover } from "./popover-placement";
import type { LookupAnchor } from "./popover-placement";
import { createSettingsAutosave } from "./settings-autosave";
import {
  asrSelectionError,
  audioSettingsChanged,
  audioSelectionErrors,
  validComputeTypes,
} from "./settings-validation";
import type {
  AsrCapabilities,
  AsrModelRecord,
  AnkiStatus,
  AudioDevice,
  ConnectionState,
  DictionaryEntry,
  DictionarySource,
  Health,
  Settings,
  Subtitle,
} from "./types";

type Page = "live" | "history" | "settings";
type SettingsCategory = "system" | "recognition" | "audio" | "dictionary" | "anki" | "debug";
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
const DEMO_VRCHAT_WARNING = demoParams.has("vrchat-warning");
const DEMO_CUDA_MISSING = demoParams.has("cuda-missing");
const NATIVE_APP = isTauri();
const CONVERSATION_STARTS_KEY = "vrcs.conversation-starts.v1";
const SIDEBAR_OPEN_KEY = "vrcs.conversation-sidebar-open";

const demoSettings: Settings = {
  schema_version: 3,
  server: { host: "127.0.0.1", port: 8766 },
  storage: { database_path: "data/vrcs.db", subtitle_history_limit: 500 },
  audio: {
    sample_rate: 16000,
    output: { mode: "system", device_id: null },
    microphone: { mode: "device", device_id: 2 },
  },
  asr: { model: "small", language: "auto", device: "auto", compute_type: "int8" },
  anki: { port: 8765, deck: "VRCS", model: "Basic", front_field: "Front", back_field: "Back" },
};

const demoAnkiStatus: AnkiStatus = {
  connected: true,
  version: 6,
  decks: [
    "Default",
    "VRCS",
    "VRCS::English",
    "VRCS::Japanese",
    "VRCS::Japanese::JLPT N5",
    "VRCS::Japanese::JLPT N4",
    "Study",
    "Study::Sentences",
  ],
  models: ["Basic", "Cloze"],
  fields: ["Front", "Back"],
  configuration_valid: true,
  error_code: null,
  message: "AnkiConnect 已连接，制卡配置有效",
};

const demoDevices: AudioDevice[] = [
  { id: 1, name: "Realtek High Definition Audio", is_default: true, is_loopback: true, sample_rate: 48000, channels: 2 },
  { id: 2, name: "Realtek Microphone Array", is_default: true, is_loopback: false, sample_rate: 48000, channels: 1 },
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

const demoAsrCapabilities: AsrCapabilities = {
  runtime_available: true,
  cuda: DEMO_CUDA_MISSING
    ? {
        available: false,
        device_count: 1,
        error: "未找到 CUDA 12 运行库：cudart64_12.dll、cublasLt64_12.dll、cublas64_12.dll",
      }
    : { available: true, device_count: 1, error: null },
  compute_types: {
    auto: DEMO_CUDA_MISSING ? ["int8"] : ["int8", "float16", "int8_float16"],
    cpu: ["int8"],
    cuda: DEMO_CUDA_MISSING ? [] : ["int8", "float16", "int8_float16"],
  },
  models: [
    { id: "tiny", repository: "Systran/faster-whisper-tiny", status: "downloaded" },
    { id: "base", repository: "Systran/faster-whisper-base", status: "downloaded" },
    { id: "small", repository: "Systran/faster-whisper-small", status: "ready" },
    { id: "medium", repository: "Systran/faster-whisper-medium", status: "not_downloaded" },
    { id: "large-v3", repository: "Systran/faster-whisper-large-v3", status: "not_downloaded" },
  ],
};

const demoModels: AsrModelRecord[] = [
  {
    id: "tiny",
    repository: "Systran/faster-whisper-tiny",
    status: "downloaded",
    active: false,
    downloaded_bytes: 75_120_000,
    total_bytes: 75_120_000,
    progress: 1,
    error: null,
  },
  {
    id: "base",
    repository: "Systran/faster-whisper-base",
    status: "downloaded",
    active: false,
    downloaded_bytes: 142_380_000,
    total_bytes: 142_380_000,
    progress: 1,
    error: null,
  },
  {
    id: "small",
    repository: "Systran/faster-whisper-small",
    status: "ready",
    active: true,
    downloaded_bytes: 466_050_000,
    total_bytes: 466_050_000,
    progress: 1,
    error: null,
  },
  {
    id: "medium",
    repository: "Systran/faster-whisper-medium",
    status: "not_downloaded",
    active: false,
    downloaded_bytes: 0,
    total_bytes: 1_530_000_000,
    progress: 0,
    error: null,
  },
  {
    id: "large-v3",
    repository: "Systran/faster-whisper-large-v3",
    status: "not_downloaded",
    active: false,
    downloaded_bytes: 0,
    total_bytes: 3_100_000_000,
    progress: 0,
    error: null,
  },
];

const MODEL_PRESENTATION: Record<AsrModelRecord["id"], {
  name: string;
  description: string;
}> = {
  tiny: { name: "Tiny", description: "启动最快，适合轻量设备和短对话" },
  base: { name: "Base", description: "低资源占用，日常语音的均衡起点" },
  small: { name: "Small", description: "速度与识别质量平衡，推荐多数设备使用" },
  medium: { name: "Medium", description: "更高识别质量，需要更多内存和显存" },
  "large-v3": { name: "Large v3", description: "最高识别质量，适合高性能显卡" },
};

function formatBytes(bytes: number): string {
  if (bytes < 1_000_000) return `${Math.max(0, Math.round(bytes / 1_000))} KB`;
  if (bytes < 1_000_000_000) return `${(bytes / 1_000_000).toFixed(bytes < 100_000_000 ? 1 : 0)} MB`;
  return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
}

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
  const [coreConfigured, setCoreConfigured] = useState(DEMO_MODE);
  const [health, setHealth] = useState<Health | null>(DEMO_MODE ? { ...demoHealth, capture_running: !DEMO_STOPPED } : null);
  const [subtitles, setSubtitles] = useState<Subtitle[]>(DEMO_MODE ? demoSubtitles : []);
  const [settings, setSettings] = useState<Settings | null>(DEMO_MODE ? demoSettings : null);
  const persistedSettingsRef = useRef<Settings | null>(DEMO_MODE ? demoSettings : null);
  const [devices, setDevices] = useState<AudioDevice[]>(DEMO_MODE ? demoDevices : []);
  const [devicesReady, setDevicesReady] = useState(DEMO_MODE);
  const [asrCapabilities, setAsrCapabilities] = useState<AsrCapabilities | null>(
    DEMO_MODE ? demoAsrCapabilities : null,
  );
  const [dictionarySources, setDictionarySources] = useState<DictionarySource[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [vrchatWarningOpen, setVrchatWarningOpen] = useState(DEMO_MODE && DEMO_VRCHAT_WARNING);
  const [cudaRuntimeWarningOpen, setCudaRuntimeWarningOpen] = useState(false);
  const cudaRuntimeWarningShownRef = useRef(false);
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
    const runtimeMissing = Boolean(
      asrCapabilities
      && asrCapabilities.cuda.device_count > 0
      && !asrCapabilities.cuda.available,
    );
    if (runtimeMissing && !cudaRuntimeWarningShownRef.current) {
      cudaRuntimeWarningShownRef.current = true;
      setCudaRuntimeWarningOpen(true);
    } else if (!runtimeMissing) {
      cudaRuntimeWarningShownRef.current = false;
      setCudaRuntimeWarningOpen(false);
    }
  }, [asrCapabilities]);

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

  useEffect(() => {
    if (DEMO_MODE) return;
    let cancelled = false;
    void initializeCoreApi()
      .then(() => {
        if (!cancelled) setCoreConfigured(true);
      })
      .catch((reason) => {
        if (!cancelled) {
          setConnection("disconnected");
          setError(reason instanceof Error ? reason.message : "Core 服务初始化失败");
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const refresh = useCallback(async () => {
    if (DEMO_MODE || !coreConfigured) return;
    try {
      const [nextHealth, nextSettings, historyItems, nextAsrCapabilities] = await Promise.all([
        coreApi.health(),
        coreApi.settings(),
        coreApi.subtitles(),
        coreApi.asrCapabilities(),
      ]);
      setHealth(nextHealth);
      persistedSettingsRef.current = nextSettings;
      setSettings(nextSettings);
      setSubtitles(historyItems);
      setAsrCapabilities(nextAsrCapabilities);
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "无法连接 Core 服务");
    }
  }, [coreConfigured]);

  useEffect(() => {
    if (DEMO_MODE || !coreConfigured) return;
    void refresh();
    const timer = window.setInterval(
      () => {
        if (settings === null) void refresh();
        else void coreApi.health().then(setHealth).catch(() => setHealth(null));
      },
      2500,
    );
    return () => window.clearInterval(timer);
  }, [coreConfigured, refresh, settings]);

  useEffect(() => {
    if (DEMO_MODE || !coreConfigured) return;
    let socket: WebSocket | null = null;
    let retry: number | null = null;
    let closed = false;
    const connect = () => {
      setConnection("connecting");
      socket = new WebSocket(coreWebSocketUrl());
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
  }, [coreConfigured]);

  const loadDevices = useCallback(async () => {
    if (DEMO_MODE || !coreConfigured) return;
    try {
      setDevices(await coreApi.devices());
      setDevicesReady(true);
      setError(null);
    } catch (reason) {
      setDevicesReady(false);
      setError(reason instanceof Error ? reason.message : "设备枚举失败");
    }
  }, [coreConfigured]);

  const loadDictionaries = useCallback(async () => {
    if (DEMO_MODE || !coreConfigured) return;
    try {
      setDictionarySources(await coreApi.dictionaries());
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "词典列表加载失败");
    }
  }, [coreConfigured]);

  const loadAsrCapabilities = useCallback(async () => {
    if (DEMO_MODE || !coreConfigured) return;
    try {
      setAsrCapabilities(await coreApi.asrCapabilities());
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "识别能力检测失败");
    }
  }, [coreConfigured]);
  const loadAsrCapabilitiesRef = useRef(loadAsrCapabilities);
  loadAsrCapabilitiesRef.current = loadAsrCapabilities;

  useEffect(() => {
    if (page === "settings") {
      void Promise.all([loadDevices(), loadDictionaries(), loadAsrCapabilities()]);
    }
  }, [loadAsrCapabilities, loadDevices, loadDictionaries, page]);

  const toggleCapture = async () => {
    if (DEMO_MODE) {
      setHealth((current) => current ? { ...current, capture_running: !current.capture_running } : current);
      return;
    }
    try {
      if (health?.capture_running) await coreApi.stop();
      else await coreApi.start();
      setHealth(await coreApi.health());
      setError(null);
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : "操作失败";
      if (shouldShowVrchatNotRunningWarning(
        message,
        settings?.audio.output.mode === "vrchat",
      )) {
        setError(null);
        setLookup(null);
        setVrchatWarningOpen(true);
        if (compact) {
          try {
            await resizeCompactWindow(true);
          } catch (resizeError) {
            setError(resizeError instanceof Error ? resizeError.message : "警告窗口展开失败");
          }
        }
      } else {
        setError(message);
      }
    }
  };

  const persistSettings = async (next: Settings): Promise<Settings> => {
    const previous = persistedSettingsRef.current;
    const restartCapture = (
      !DEMO_MODE
      && Boolean(health?.capture_running)
      && previous !== null
      && audioSettingsChanged(previous, next)
    );
    let captureStopped = false;
    let saved: Settings | null = null;

    try {
      if (restartCapture) {
        await coreApi.stop();
        captureStopped = true;
      }
      saved = DEMO_MODE ? next : await coreApi.saveSettings(next);
      if (restartCapture) {
        await coreApi.start();
        void coreApi.health().then(setHealth).catch(() => undefined);
      }
      persistedSettingsRef.current = saved;
      return saved;
    } catch (reason) {
      if (restartCapture && captureStopped) {
        let recoveryError: unknown = null;
        if (saved !== null && previous !== null) {
          try {
            await coreApi.saveSettings(previous);
          } catch (rollbackReason) {
            recoveryError = rollbackReason;
          }
        }
        try {
          await coreApi.start();
          void coreApi.health().then(setHealth).catch(() => undefined);
        } catch (restartReason) {
          recoveryError ??= restartReason;
        }
        if (recoveryError) {
          const applyMessage = reason instanceof Error ? reason.message : "设置应用失败";
          const recoveryMessage = recoveryError instanceof Error
            ? recoveryError.message
            : "未知错误";
          throw new Error(
            `${applyMessage}；恢复旧配置或旧采集失败：${recoveryMessage}`,
            { cause: reason },
          );
        }
      }
      throw reason;
    }
  };
  const persistSettingsRef = useRef(persistSettings);
  persistSettingsRef.current = persistSettings;
  const settingsAutosaveRef = useRef<ReturnType<typeof createSettingsAutosave<Settings>> | null>(null);
  if (settingsAutosaveRef.current === null) {
    settingsAutosaveRef.current = createSettingsAutosave<Settings>({
      persist: (next) => persistSettingsRef.current(next),
      onOptimistic: setSettings,
      onCommit: (saved) => {
        persistedSettingsRef.current = saved;
        setSettings(saved);
        setError(null);
        void loadAsrCapabilitiesRef.current();
      },
      onError: (reason) => {
        if (persistedSettingsRef.current) setSettings(persistedSettingsRef.current);
        setError(reason instanceof Error ? reason.message : "设置应用失败");
      },
    });
  }
  const saveSettings = settingsAutosaveRef.current;

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

  const closeVrchatWarning = () => {
    setVrchatWarningOpen(false);
    if (compact) {
      void resizeCompactWindow(false).catch((reason) => {
        setError(reason instanceof Error ? reason.message : "小窗收起失败");
      });
    }
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
        {vrchatWarningOpen && <VrchatNotRunningDialog onClose={closeVrchatWarning} />}
        {cudaRuntimeWarningOpen && <CudaRuntimeDialog onClose={() => setCudaRuntimeWarningOpen(false)} />}
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
              devicesReady={devicesReady}
              dictionaries={dictionarySources}
              disabled={health?.capture_running ?? false}
              modelStatus={health?.asr_status ?? "unknown"}
              asrCapabilities={asrCapabilities}
              onRefresh={loadDevices}
              onImportDictionary={importDictionary}
              onDeleteDictionary={deleteDictionary}
              onModelsChanged={loadAsrCapabilities}
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
      {vrchatWarningOpen && <VrchatNotRunningDialog onClose={closeVrchatWarning} />}
      {cudaRuntimeWarningOpen && <CudaRuntimeDialog onClose={() => setCudaRuntimeWarningOpen(false)} />}
    </div>
  );
}

function VrchatNotRunningDialog({ onClose }: { onClose: () => void }) {
  return (
    <WarningDialog
      id="vrchat-warning"
      title="未检测到 VRChat"
      description="请先启动 VRChat，等待程序完成加载后再开始转写。"
      onClose={onClose}
    />
  );
}

function CudaRuntimeDialog({ onClose }: { onClose: () => void }) {
  return (
    <WarningDialog
      id="cuda-runtime-warning"
      title="缺少 CUDA 12 运行库"
      description="检测到 NVIDIA GPU，但无法加载 CUDA 12 的 cuBLAS 运行库。请安装 CUDA 12.x Runtime，完成后重新启动 VRCS。"
      onClose={onClose}
    />
  );
}

function WarningDialog({ id, title, description, onClose }: {
  id: string;
  title: string;
  description: string;
  onClose: () => void;
}) {
  const confirmRef = useRef<HTMLButtonElement>(null);
  const onCloseRef = useRef(onClose);

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    const previousFocus = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    confirmRef.current?.focus();
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCloseRef.current();
      if (event.key === "Tab") {
        event.preventDefault();
        confirmRef.current?.focus();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      previousFocus?.focus();
    };
  }, []);

  return (
    <div
      className="warning-dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section
        className="warning-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={`${id}-title`}
        aria-describedby={`${id}-description`}
      >
        <div className="warning-dialog-icon" aria-hidden="true"><TriangleAlert size={22} /></div>
        <div className="warning-dialog-copy">
          <h2 id={`${id}-title`}>{title}</h2>
          <p id={`${id}-description`}>{description}</p>
        </div>
        <button ref={confirmRef} className="primary-button" type="button" onClick={onClose}>我知道了</button>
      </section>
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

function SettingsPanel({ settings, devices, devicesReady, dictionaries, disabled, modelStatus, asrCapabilities, onRefresh, onImportDictionary, onDeleteDictionary, onModelsChanged, onSave }: {
  settings: Settings;
  devices: AudioDevice[];
  devicesReady: boolean;
  dictionaries: DictionarySource[];
  disabled: boolean;
  modelStatus: string;
  asrCapabilities: AsrCapabilities | null;
  onRefresh: () => Promise<void>;
  onImportDictionary: (file: File) => Promise<DictionarySource>;
  onDeleteDictionary: (id: number) => Promise<void>;
  onModelsChanged: () => Promise<void>;
  onSave: (value: Settings) => Promise<Settings>;
}) {
  const [draft, setDraft] = useState(settings);
  const [activeCategory, setActiveCategory] = useState<SettingsCategory>("system");
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const [saveMessage, setSaveMessage] = useState("");
  const [desktopPreferences, setDesktopPreferences] = useState(defaultDesktopPreferences);
  const [desktopPreferencesReady, setDesktopPreferencesReady] = useState(false);
  const [desktopSaveState, setDesktopSaveState] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const [desktopMessage, setDesktopMessage] = useState("");
  const [dictionaryBusy, setDictionaryBusy] = useState(false);
  const [dictionaryMessage, setDictionaryMessage] = useState("");
  const [managedModels, setManagedModels] = useState<AsrModelRecord[]>(DEMO_MODE ? demoModels : []);
  const [modelsReady, setModelsReady] = useState(DEMO_MODE);
  const [modelMessage, setModelMessage] = useState("");
  const [ankiStatus, setAnkiStatus] = useState<AnkiStatus | null>(DEMO_MODE ? demoAnkiStatus : null);
  const [ankiBusy, setAnkiBusy] = useState(false);
  const [ankiMessage, setAnkiMessage] = useState("");
  const [ankiPortText, setAnkiPortText] = useState(String(settings.anki.port));
  const [ankiPortError, setAnkiPortError] = useState("");
  const dictionaryFileRef = useRef<HTMLInputElement>(null);
  const draftRef = useRef(settings);
  const saveVersionRef = useRef(0);
  const managedModelsRef = useRef(managedModels);
  managedModelsRef.current = managedModels;
  useEffect(() => {
    draftRef.current = settings;
    setDraft(settings);
    setAnkiPortText(String(settings.anki.port));
  }, [settings]);
  useEffect(() => {
    let cancelled = false;
    void loadDesktopPreferences().then(
      (saved) => {
        if (cancelled) return;
        setDesktopPreferences(saved);
        setDesktopPreferencesReady(true);
      },
      (reason) => {
        if (cancelled) return;
        setDesktopMessage(reason instanceof Error ? reason.message : "系统设置读取失败");
        setDesktopSaveState("error");
        setDesktopPreferencesReady(true);
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);
  const loadModels = useCallback(async () => {
    if (DEMO_MODE) return;
    try {
      const previous = managedModelsRef.current;
      const next = await coreApi.asrModels();
      managedModelsRef.current = next;
      setManagedModels(next);
      setModelsReady(true);
      if (
        previous.some((model) => model.status === "downloading")
        && !next.some((model) => model.status === "downloading")
      ) {
        void onModelsChanged();
      }
    } catch (reason) {
      setModelsReady(false);
      setModelMessage(reason instanceof Error ? reason.message : "模型列表读取失败");
    }
  }, [onModelsChanged]);
  useEffect(() => {
    void loadModels();
  }, [loadModels]);
  useEffect(() => {
    if (DEMO_MODE || activeCategory !== "recognition") return;
    const timer = window.setInterval(() => void loadModels(), 750);
    return () => window.clearInterval(timer);
  }, [activeCategory, loadModels]);
  const loadAnkiStatus = useCallback(async () => {
    setAnkiBusy(true);
    setAnkiMessage("");
    try {
      const next = DEMO_MODE ? demoAnkiStatus : await coreApi.ankiStatus();
      setAnkiStatus(next);
      setAnkiMessage(next.message);
    } catch (reason) {
      setAnkiStatus(null);
      setAnkiMessage(reason instanceof Error ? reason.message : "AnkiConnect 检测失败");
    } finally {
      setAnkiBusy(false);
    }
  }, []);
  useEffect(() => {
    if (activeCategory === "anki") void loadAnkiStatus();
  }, [activeCategory, loadAnkiStatus]);
  const applySettings = (
    update: (current: Settings) => Settings,
    afterSave?: () => void,
  ) => {
    const next = update(draftRef.current);
    const version = ++saveVersionRef.current;
    draftRef.current = next;
    setDraft(next);
    setSaveState("saving");
    setSaveMessage("");
    void onSave(next).then(
      (saved) => {
        if (version !== saveVersionRef.current) return;
        draftRef.current = saved;
        setDraft(saved);
        setSaveState("saved");
        afterSave?.();
      },
      (reason) => {
        if (version !== saveVersionRef.current) return;
        setSaveMessage(reason instanceof Error ? reason.message : "设置应用失败");
        setSaveState("error");
      },
    );
  };
  const updateAnki = <K extends keyof Settings["anki"]>(
    key: K,
    value: Settings["anki"][K],
  ) => {
    applySettings(
      (current) => ({ ...current, anki: { ...current.anki, [key]: value } }),
      () => void loadAnkiStatus(),
    );
  };
  const commitAnkiPort = () => {
    const port = Number(ankiPortText);
    if (!Number.isInteger(port) || port < 1 || port > 65_535) {
      setAnkiPortError("请输入 1 到 65535 之间的端口");
      return;
    }
    setAnkiPortError("");
    if (port !== draftRef.current.anki.port) updateAnki("port", port);
  };
  const updateAsr = <K extends keyof Settings["asr"]>(key: K, value: Settings["asr"][K]) => {
    applySettings((current) => {
      const nextAsr = { ...current.asr, [key]: value };
      if (key === "device") {
        const allowed = validComputeTypes(asrCapabilities, nextAsr.device);
        if (!allowed.includes(nextAsr.compute_type)) {
          nextAsr.compute_type = allowed[0] ?? "int8";
        }
      }
      return { ...current, asr: nextAsr };
    });
  };
  const updateDesktop = async (key: keyof DesktopPreferences, enabled: boolean) => {
    const previous = desktopPreferences;
    const optimistic = { ...previous, [key]: enabled };
    setDesktopPreferences(optimistic);
    setDesktopSaveState("saving");
    setDesktopMessage("");
    try {
      const saved = await updateDesktopPreference(previous, key, enabled);
      setDesktopPreferences(saved);
      setDesktopSaveState("saved");
    } catch (reason) {
      setDesktopPreferences(previous);
      setDesktopMessage(reason instanceof Error ? reason.message : "系统设置保存失败");
      setDesktopSaveState("error");
    }
  };
  const outputDevices = devices.filter((device) => device.is_loopback);
  const microphoneDevices = devices.filter((device) => !device.is_loopback);
  const deviceErrors = devicesReady ? audioSelectionErrors(draft, devices) : [];
  const asrError = asrSelectionError(draft, asrCapabilities);
  const validationError = deviceErrors[0] ?? asrError;
  const computeTypes = validComputeTypes(asrCapabilities, draft.asr.device);
  const selectedModelCapability = asrCapabilities?.models.find(
    (model) => model.id === draft.asr.model,
  );
  const selectedModelStatus = (
    draft.asr.model === settings.asr.model
    && ["loading", "ready", "error"].includes(modelStatus)
  )
    ? modelStatus
    : selectedModelCapability?.status;
  const modelStatusLabel = selectedModelStatus === "not_downloaded"
    ? "请先在模型管理器中下载"
    : selectedModelStatus === "loading"
      ? "正在下载或加载模型"
      : selectedModelStatus === "error"
        ? "模型加载失败"
        : selectedModelStatus
          ? "模型文件已就绪"
          : "正在检查模型文件";
  const installedModels = managedModels.filter((model) =>
    ["downloaded", "loading", "ready"].includes(model.status),
  );
  const downloadingModels = managedModels.filter((model) => model.status === "downloading");
  const selectableModels = modelsReady
    ? managedModels.filter((model) =>
        model.id === draft.asr.model
        || ["downloaded", "loading", "ready"].includes(model.status),
      )
    : (asrCapabilities?.models ?? demoAsrCapabilities.models).filter((model) =>
        model.id === draft.asr.model || model.status !== "not_downloaded",
      );
  const ankiOptionList = (values: string[], current: string) => (
    Array.from(new Set([current, ...values])).map((value) => ({ value, label: value }))
  );
  const ankiDeckNames = useMemo(
    () => Array.from(new Set([draft.anki.deck, ...(ankiStatus?.decks ?? [])])),
    [ankiStatus?.decks, draft.anki.deck],
  );
  const ankiModelOptions = ankiOptionList(ankiStatus?.models ?? [], draft.anki.model);
  const ankiFieldOptions = ankiOptionList(
    (ankiStatus?.fields ?? []).filter((field) => field !== draft.anki.back_field),
    draft.anki.front_field,
  );
  const ankiBackFieldOptions = ankiOptionList(
    (ankiStatus?.fields ?? []).filter((field) => field !== draft.anki.front_field),
    draft.anki.back_field,
  );
  const settingsCategories: Array<{ id: SettingsCategory; label: string; icon: React.ReactNode }> = [
    { id: "system", label: "常规", icon: <SlidersHorizontal size={18} /> },
    { id: "audio", label: "音频", icon: <Volume2 size={18} /> },
    { id: "recognition", label: "识别", icon: <Languages size={18} /> },
    { id: "dictionary", label: "词典", icon: <BookOpen size={18} /> },
    { id: "anki", label: "Anki", icon: <PlusCircle size={18} /> },
    { id: "debug", label: "Debug", icon: <Wrench size={18} /> },
  ];
  const debugRows = [
    { label: "配置 Schema", value: `v${draft.schema_version}` },
    { label: "Core 地址", value: `${draft.server.host}:${draft.server.port}` },
    { label: "数据库路径", value: draft.storage.database_path },
    { label: "采样率", value: `${draft.audio.sample_rate.toLocaleString("zh-CN")} Hz` },
    { label: "字幕保留上限", value: `${draft.storage.subtitle_history_limit.toLocaleString("zh-CN")} 条` },
    { label: "识别模型状态", value: modelStatus },
    { label: "CUDA 预检", value: asrCapabilities?.cuda.available ? `${asrCapabilities.cuda.device_count} 个可用设备` : "不可用" },
    { label: "转写状态", value: disabled ? "正在转写" : "已停止" },
    { label: "音频设备", value: `${outputDevices.length} 个系统输出，${microphoneDevices.length} 个麦克风输入` },
    { label: "词典数量", value: dictionaries.length ? `${dictionaries.length} 部` : "尚未导入" },
  ];
  const settingsActionText = activeCategory === "dictionary"
    ? "词典导入和移除会立即生效"
    : activeCategory === "anki"
      ? ankiPortError
        || (saveState === "saving"
          ? "正在保存 Anki 设置…"
          : saveState === "error"
            ? saveMessage || "Anki 设置保存失败"
            : ankiMessage || "Anki 设置保存后会自动重新检测连接")
    : activeCategory === "system"
      ? !desktopPreferencesReady
        ? "正在读取 Windows 启动与托盘设置…"
        : desktopSaveState === "saving"
          ? "正在保存系统设置…"
          : desktopSaveState === "saved"
            ? "系统设置已保存"
            : desktopSaveState === "error"
              ? desktopMessage || "系统设置保存失败，请稍后重试"
              : "这些设置只影响桌面应用，不会自动开始转写"
    : activeCategory === "debug"
      ? "Debug 信息用于排查本地服务、配置与采集状态"
      : validationError
          ? validationError
          : saveState === "saving"
        ? "正在应用设置…"
        : saveState === "saved"
          ? "设置已应用"
          : saveState === "error"
            ? saveMessage || "应用失败，原设置仍然有效"
            : "修改后立即校验并应用";
  const visibleSaveState = activeCategory === "system"
    ? desktopSaveState
    : activeCategory === "anki" && (ankiPortError || (ankiStatus && !ankiStatus.configuration_valid))
      ? "error"
      : saveState;
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
  const downloadModel = async (model: AsrModelRecord) => {
    setModelMessage(`正在准备下载 ${MODEL_PRESENTATION[model.id].name}…`);
    if (DEMO_MODE) {
      setManagedModels((current) => current.map((item) => (
        item.id === model.id
          ? {
              ...item,
              status: "downloading",
              progress: 0.04,
              downloaded_bytes: Math.round(item.total_bytes * 0.04),
              error: null,
            }
          : item
      )));
      for (let step = 1; step <= 8; step += 1) {
        await new Promise((resolve) => window.setTimeout(resolve, 120));
        const progress = Math.min(0.08 + step * 0.115, 0.99);
        setManagedModels((current) => current.map((item) => (
          item.id === model.id
            ? {
                ...item,
                progress,
                downloaded_bytes: Math.round(item.total_bytes * progress),
              }
            : item
        )));
      }
      setManagedModels((current) => current.map((item) => (
        item.id === model.id
          ? {
              ...item,
              status: "downloaded",
              progress: 1,
              downloaded_bytes: item.total_bytes,
            }
          : item
      )));
      setModelMessage(`${MODEL_PRESENTATION[model.id].name} 已下载`);
      return;
    }
    try {
      await coreApi.downloadAsrModel(model.id);
      setModelMessage(`${MODEL_PRESENTATION[model.id].name} 已加入下载队列`);
      await loadModels();
    } catch (reason) {
      setModelMessage(reason instanceof Error ? reason.message : "模型下载启动失败");
    }
  };
  const removeModel = async (model: AsrModelRecord) => {
    const name = MODEL_PRESENTATION[model.id].name;
    if (!window.confirm(`确定删除 ${name} 模型吗？以后仍可重新下载。`)) return;
    setModelMessage(`正在删除 ${name}…`);
    if (DEMO_MODE) {
      setManagedModels((current) => current.map((item) => (
        item.id === model.id
          ? {
              ...item,
              status: "not_downloaded",
              progress: 0,
              downloaded_bytes: 0,
            }
          : item
      )));
      setModelMessage(`${name} 已从本机删除`);
      return;
    }
    try {
      await coreApi.deleteAsrModel(model.id);
      await loadModels();
      await onModelsChanged();
      setModelMessage(`${name} 已从本机删除`);
    } catch (reason) {
      setModelMessage(reason instanceof Error ? reason.message : "模型删除失败");
    }
  };

  return (
    <section className="settings-surface">
      <div className="settings-tabbar-wrap">
        <div className="settings-tabbar" role="tablist" aria-label="设置分类">
          {settingsCategories.map((category) => {
            const active = activeCategory === category.id;
            return (
              <button
                key={category.id}
                id={`settings-tab-${category.id}`}
                className={active ? "active" : ""}
                type="button"
                role="tab"
                aria-selected={active}
                aria-controls={`settings-panel-${category.id}`}
                aria-label={category.label}
                onClick={() => setActiveCategory(category.id)}
              >
                <span className="settings-tab-icon" aria-hidden="true">{category.icon}</span>
                <span className="settings-tab-label">{category.label}</span>
              </button>
            );
          })}
        </div>
      </div>

      {activeCategory === "system" && (
        <div className="settings-section settings-section-active system-section" id="settings-panel-system" role="tabpanel" aria-labelledby="settings-tab-system">
          <div className="section-heading">
            <div><SlidersHorizontal size={18} /><h2>常规</h2><span>Windows 应用行为</span></div>
            <p>修改后立即保存</p>
          </div>
          <div className="settings-toggle-list">
            <PreferenceToggle
              title="开机时启动 VRCS"
              description="登录 Windows 后自动启动应用，但不会自动开始转写。"
              checked={desktopPreferences.launchAtStartup}
              disabled={!desktopPreferencesReady || desktopSaveState === "saving"}
              onChange={(enabled) => void updateDesktop("launchAtStartup", enabled)}
            />
            <PreferenceToggle
              title="关闭时最小化到系统托盘"
              description="点击关闭按钮时隐藏主窗口，转写和 Core 服务会继续运行。"
              checked={desktopPreferences.minimizeToTray}
              disabled={!desktopPreferencesReady || desktopSaveState === "saving"}
              onChange={(enabled) => void updateDesktop("minimizeToTray", enabled)}
            />
          </div>
        </div>
      )}

      {activeCategory === "recognition" && (
        <div className="settings-section settings-section-active recognition-section" id="settings-panel-recognition" role="tabpanel" aria-labelledby="settings-tab-recognition">
          <div className="section-heading">
            <div><Languages size={18} /><h2>识别引擎</h2><span className="status-chip">状态：{modelStatus}</span></div>
            <p>{disabled ? "停止转写后可修改" : "修改后立即应用"}</p>
          </div>
          <div className={`recognition-runtime ${asrCapabilities?.cuda.available ? "available" : "unavailable"}`}>
            <span className="recognition-runtime-dot" aria-hidden="true" />
            <div>
              <strong>运行环境</strong>
              <span>
                {asrCapabilities === null
                  ? "正在检测运行环境…"
                  : asrCapabilities.cuda.available
                    ? `已发现 ${asrCapabilities.cuda.device_count} 个 CUDA 设备，可选择 GPU 加速`
                    : asrCapabilities.cuda.device_count > 0
                      ? "已检测到 NVIDIA GPU，但 CUDA 12 运行库不可用"
                      : "未发现可用 CUDA，已过滤 GPU 专用组合"}
              </span>
            </div>
          </div>
          <div className="recognition-config">
            <div className="recognition-config-row">
              <div className="recognition-config-title">
                <Languages size={17} />
                <span><strong>识别内容</strong><small>选择模型大小与输入语言</small></span>
              </div>
              <div className="recognition-config-fields">
                <Select
                  label="模型"
                  helper={modelStatusLabel}
                  value={draft.asr.model}
                  options={selectableModels.map((model) => ({
                    value: model.id,
                    label: `${model.id} · ${
                      model.status === "not_downloaded"
                        ? "未下载"
                        : model.status === "loading"
                          ? "加载中"
                          : model.status === "error"
                            ? "错误"
                            : "已就绪"
                    }`,
                  }))}
                  disabled={disabled}
                  onChange={(value) => updateAsr("model", value as Settings["asr"]["model"])}
                />
                <Select label="语言" helper="保留原语言转写，不翻译" value={draft.asr.language} values={["auto", "en", "ja", "zh", "ko", "es", "fr", "de"]} disabled={disabled} onChange={(value) => updateAsr("language", value as Settings["asr"]["language"])} />
              </div>
            </div>
            <div className="recognition-config-row">
              <div className="recognition-config-title">
                <HardDrive size={17} />
                <span><strong>运行方式</strong><small>按硬件能力过滤有效组合</small></span>
              </div>
              <div className="recognition-config-fields">
                <Select
                  label="运行设备"
                  helper={asrError ?? "只显示通过预检的设备"}
                  value={draft.asr.device}
                  options={[
                    { value: "auto", label: "自动选择" },
                    { value: "cpu", label: "CPU" },
                    ...(asrCapabilities?.cuda.available ? [{ value: "cuda", label: "CUDA" }] : []),
                    ...(draft.asr.device === "cuda" && !asrCapabilities?.cuda.available
                      ? [{ value: "cuda", label: "CUDA · 不可用" }]
                      : []),
                  ]}
                  disabled={disabled}
                  onChange={(value) => updateAsr("device", value as Settings["asr"]["device"])}
                />
                <Select
                  label="计算类型"
                  helper="根据运行设备过滤"
                  value={draft.asr.compute_type}
                  values={computeTypes}
                  disabled={disabled}
                  onChange={(value) => updateAsr("compute_type", value as Settings["asr"]["compute_type"])}
                />
              </div>
            </div>
          </div>
          <section className="model-section recognition-models" aria-labelledby="local-models-heading">
          <div className="section-heading">
            <div>
              <HardDrive size={18} />
              <h2 id="local-models-heading">本地模型</h2>
              <span>
                {downloadingModels.length
                  ? `${downloadingModels.length} 个下载中`
                  : modelsReady
                    ? `已安装 ${installedModels.length} 个`
                    : "正在读取"}
              </span>
            </div>
            <button className="secondary-button" type="button" disabled={!modelsReady} onClick={() => void loadModels()}><RefreshCw size={15} />刷新</button>
          </div>

          {!modelsReady && managedModels.length === 0 ? (
            <div className="model-list-pending" role="status">
              <RefreshCw size={17} />
              <span>正在检查本地模型文件…</span>
            </div>
          ) : (
            <div className="model-list">
              {managedModels.map((model) => {
                const presentation = MODEL_PRESENTATION[model.id];
                const downloaded = ["downloaded", "loading", "ready"].includes(model.status);
                const downloading = model.status === "downloading";
                const percentage = Math.round(model.progress * 100);
                const sizeLabel = downloading
                  ? `${formatBytes(model.downloaded_bytes)} / ${formatBytes(model.total_bytes)}`
                  : formatBytes(model.total_bytes);
                return (
                  <article className={`model-row model-status-${model.status}`} key={model.id}>
                    <div className="model-row-body">
                      <div className="model-row-title">
                        <strong>{presentation.name}</strong>
                        {model.active && <span className="model-active-chip">使用中</span>}
                        <span className="model-size">{sizeLabel}</span>
                      </div>
                      <p>{presentation.description}</p>
                      <code>{model.repository}</code>
                      {downloading && (
                        <div className="model-progress-wrap">
                          <div
                            className="model-progress-track"
                            role="progressbar"
                            aria-label={`${presentation.name} 下载进度`}
                            aria-valuemin={0}
                            aria-valuemax={100}
                            aria-valuenow={percentage}
                          >
                            <span style={{ transform: `scaleX(${Math.max(0.02, model.progress)})` }} />
                          </div>
                          <span>{percentage}%</span>
                        </div>
                      )}
                      {model.status === "error" && model.error && (
                        <p className="model-error" role="alert">{model.error}</p>
                      )}
                    </div>
                    <div className="model-row-action">
                      {downloading ? (
                        <span className="model-download-state"><RefreshCw size={15} />下载中</span>
                      ) : downloaded ? (
                        model.active ? (
                          <span className="model-ready-state"><Check size={15} />已就绪</span>
                        ) : (
                          <button className="model-delete-button" type="button" aria-label={`删除 ${presentation.name}`} onClick={() => void removeModel(model)}><Trash2 size={16} /><span>删除</span></button>
                        )
                      ) : (
                        <button className="model-download-button" type="button" onClick={() => void downloadModel(model)}><Download size={16} />{model.status === "error" ? "重试" : "下载"}</button>
                      )}
                    </div>
                  </article>
                );
              })}
            </div>
          )}
          {modelMessage && <p className="model-manager-feedback" role="status">{modelMessage}</p>}
          </section>
        </div>
      )}

      {activeCategory === "audio" && (
        <div className="settings-section settings-section-active audio-section" id="settings-panel-audio" role="tabpanel" aria-labelledby="settings-tab-audio">
          <div className="section-heading">
            <div><h2>音频来源</h2><span>{devices.length ? `已发现 ${devices.length} 个设备` : "等待扫描"}</span></div>
            <button className="secondary-button" type="button" onClick={() => void onRefresh()}><RefreshCw size={15} />重新扫描</button>
          </div>

          {deviceErrors.map((message) => (
            <p className="settings-validation-error" role="alert" key={message}>
              <TriangleAlert size={15} />{message}
            </p>
          ))}
          <DeviceGroup
            icon={<Volume2 size={18} />}
            title="对方声音"
            note="选择完整系统输出、仅 VRChat，或指定输出设备"
            devices={outputDevices}
            devicesReady={devicesReady}
            selectedDeviceId={draft.audio.output.mode === "system" ? draft.audio.output.device_id : null}
            specialRows={[
              {
                key: "system",
                name: "系统输出",
                description: "采集 Windows 默认输出设备的全部声音",
                chosen: draft.audio.output.mode === "system" && draft.audio.output.device_id === null,
                onSelect: () => applySettings((current) => ({
                  ...current,
                  audio: { ...current.audio, output: { mode: "system", device_id: null } },
                })),
              },
              {
                key: "vrchat",
                name: "VRChat",
                description: "仅采集 VRChat.exe，排除浏览器和系统提示音",
                chosen: draft.audio.output.mode === "vrchat",
                onSelect: () => applySettings((current) => ({
                  ...current,
                  audio: { ...current.audio, output: { mode: "vrchat", device_id: null } },
                })),
              },
            ]}
            disabled={saveState === "saving"}
            onSelectDevice={(id) => applySettings((current) => ({
              ...current,
              audio: { ...current.audio, output: { mode: "system", device_id: id } },
            }))}
          />
          <DeviceGroup
            icon={<Mic size={18} />}
            title="自己的声音"
            note="默认麦克风会跟随 Windows，关闭后仅转写对方声音"
            devices={microphoneDevices}
            devicesReady={devicesReady}
            selectedDeviceId={draft.audio.microphone.mode === "device" ? draft.audio.microphone.device_id : null}
            specialRows={[
              {
                key: "default",
                name: "默认麦克风",
                description: "跟随 Windows 默认输入设备",
                chosen: draft.audio.microphone.mode === "default",
                onSelect: () => applySettings((current) => ({
                  ...current,
                  audio: { ...current.audio, microphone: { mode: "default", device_id: null } },
                })),
              },
              {
                key: "disabled",
                name: "关闭麦克风",
                description: "不采集自己的声音",
                chosen: draft.audio.microphone.mode === "disabled",
                onSelect: () => applySettings((current) => ({
                  ...current,
                  audio: { ...current.audio, microphone: { mode: "disabled", device_id: null } },
                })),
              },
            ]}
            disabled={saveState === "saving"}
            onSelectDevice={(id) => applySettings((current) => ({
              ...current,
              audio: { ...current.audio, microphone: { mode: "device", device_id: id } },
            }))}
          />
        </div>
      )}

      {activeCategory === "dictionary" && (
        <div className="settings-section settings-section-active dictionary-section" id="settings-panel-dictionary" role="tabpanel" aria-labelledby="settings-tab-dictionary">
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
      )}

      {activeCategory === "anki" && (
        <div className="settings-section settings-section-active anki-section" id="settings-panel-anki" role="tabpanel" aria-labelledby="settings-tab-anki">
          <div className="section-heading">
            <div>
              <PlusCircle size={18} />
              <h2>Anki 制卡</h2>
              <span>本地 AnkiConnect</span>
            </div>
            <button className="secondary-button" type="button" disabled={ankiBusy} onClick={() => void loadAnkiStatus()}>
              <RefreshCw className={ankiBusy ? "spin" : ""} size={15} />
              {ankiBusy ? "检测中" : "重新检测"}
            </button>
          </div>

          <div className={`anki-connection ${ankiBusy ? "checking" : ankiStatus?.connected ? (ankiStatus.configuration_valid ? "ready" : "needs-setup") : "offline"}`} aria-live="polite">
            <span className="anki-connection-dot" aria-hidden="true" />
            <div>
              <strong>
                {ankiBusy
                  ? "正在检测 AnkiConnect"
                  : ankiStatus?.connected
                    ? ankiStatus.configuration_valid
                      ? "可以制卡"
                      : "已连接，需要完成配置"
                    : "AnkiConnect 未连接"}
              </strong>
              <p>{ankiMessage || ankiStatus?.message || "启动 Anki 后重新检测连接"}</p>
            </div>
            <code>
              {ankiStatus?.version ? `API v${ankiStatus.version}` : `127.0.0.1:${draft.anki.port}`}
            </code>
          </div>

          <div className="anki-endpoint-row">
            <div>
              <span>连接地址</span>
              <strong>127.0.0.1</strong>
              <small>仅允许访问本机，避免把制卡内容发送到远程服务</small>
            </div>
            <label className="anki-port-field">
              <span>端口</span>
              <input
                type="text"
                inputMode="numeric"
                value={ankiPortText}
                disabled={saveState === "saving"}
                aria-invalid={Boolean(ankiPortError)}
                aria-describedby="anki-port-help"
                onChange={(event) => setAnkiPortText(event.target.value.replace(/\D/g, "").slice(0, 5))}
                onBlur={commitAnkiPort}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    event.currentTarget.blur();
                  }
                }}
              />
            </label>
          </div>
          <p id="anki-port-help" className={`anki-port-help ${ankiPortError ? "error" : ""}`}>
            {ankiPortError || "AnkiConnect 默认使用 8765，一般无需修改。仅在插件配置了其他端口时调整。"}
          </p>

          <div className="form-grid anki-mapping-grid">
            <DeckTreeSelect
              label="牌组"
              helper={ankiStatus?.connected ? "新笔记会保存到此牌组" : "连接后读取可用牌组"}
              value={draft.anki.deck}
              decks={ankiDeckNames}
              disabled={!ankiStatus?.connected || ankiBusy || saveState === "saving"}
              onChange={(value) => updateAnki("deck", value)}
            />
            <Select
              label="笔记类型"
              helper={ankiStatus?.connected ? "选择 Anki 中已有的笔记类型" : "连接后读取笔记类型"}
              value={draft.anki.model}
              options={ankiModelOptions}
              disabled={!ankiStatus?.connected || ankiBusy || saveState === "saving"}
              onChange={(value) => updateAnki("model", value)}
            />
            <Select
              label="正面字段"
              helper="写入词条和读音"
              value={draft.anki.front_field}
              options={ankiFieldOptions}
              disabled={!ankiStatus?.connected || !ankiStatus.fields.length || ankiBusy || saveState === "saving"}
              onChange={(value) => updateAnki("front_field", value)}
            />
            <Select
              label="背面字段"
              helper="写入释义、语境和词典来源"
              value={draft.anki.back_field}
              options={ankiBackFieldOptions}
              disabled={!ankiStatus?.connected || !ankiStatus.fields.length || ankiBusy || saveState === "saving"}
              onChange={(value) => updateAnki("back_field", value)}
            />
          </div>
        </div>
      )}

      {activeCategory === "debug" && (
        <div className="settings-section settings-section-active debug-section" id="settings-panel-debug" role="tabpanel" aria-labelledby="settings-tab-debug">
          <div className="section-heading">
            <div><Wrench size={18} /><h2>Debug</h2><span>运行信息与诊断</span></div>
            <p>只读，不会修改配置</p>
          </div>
          <div className="debug-list">
            {debugRows.map((row) => (
              <div className="debug-row" key={row.label}>
                <span>{row.label}</span>
                <strong>{row.value}</strong>
              </div>
            ))}
          </div>
        </div>
      )}

      <div className={`settings-actions save-state-${visibleSaveState}`}>
        <span role="status" aria-live="polite">{settingsActionText}</span>
      </div>
    </section>
  );
}

function PreferenceToggle({ title, description, checked, disabled, onChange }: {
  title: string;
  description: string;
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <div className={`settings-toggle-row ${disabled ? "disabled" : ""}`}>
      <span className="settings-toggle-copy">
        <strong>{title}</strong>
        <small>{description}</small>
      </span>
      <button
        className="settings-switch-button"
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={title}
        disabled={disabled}
        onClick={() => onChange(!checked)}
      >
        <span className="switch-track" aria-hidden="true"><span /></span>
      </button>
    </div>
  );
}

function Select({ label, helper, value, values = [], options, disabled, onChange }: {
  label: string;
  helper?: string;
  value: string;
  values?: readonly string[];
  options?: Array<{ value: string; label: string }>;
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <div className="field">
      <span>{label}</span>
      <DropdownField
        label={label}
        value={value}
        options={options ?? values.map((item) => ({ value: item, label: item }))}
        disabled={disabled}
        onChange={onChange}
      />
      {helper && <small>{helper}</small>}
    </div>
  );
}

function DeckTreeSelect({ label, helper, value, decks, disabled, onChange }: {
  label: string;
  helper?: string;
  value: string;
  decks: string[];
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [expandedNames, setExpandedNames] = useState<Set<string>>(
    () => new Set(ankiDeckAncestors(value)),
  );
  const [activeName, setActiveName] = useState(value);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const itemRefs = useRef(new Map<string, HTMLButtonElement>());
  const tree = useMemo(() => buildAnkiDeckTree(decks), [decks]);
  const visibleNodes = useMemo(
    () => visibleAnkiDeckNodes(tree, expandedNames),
    [tree, expandedNames],
  );

  const closeAndFocusTrigger = () => {
    setOpen(false);
    requestAnimationFrame(() => triggerRef.current?.focus());
  };

  const openTree = () => {
    setExpandedNames((current) => new Set([
      ...current,
      ...ankiDeckAncestors(value),
    ]));
    setActiveName(value || tree[0]?.name || "");
    setOpen(true);
  };

  const toggleExpanded = (name: string) => {
    setExpandedNames((current) => {
      const next = new Set(current);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  };

  const choose = (name: string) => {
    onChange(name);
    setActiveName(name);
    closeAndFocusTrigger();
  };

  useEffect(() => {
    if (!open) return;
    const closeOnOutside = (event: PointerEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) setOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        closeAndFocusTrigger();
      }
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

  useEffect(() => {
    if (!open) return;
    const frame = requestAnimationFrame(() => itemRefs.current.get(activeName)?.focus());
    return () => cancelAnimationFrame(frame);
  }, [activeName, open]);

  return (
    <div className="field">
      <span>{label}</span>
      <div className={`deck-tree-field ${open ? "open" : ""}`} ref={rootRef}>
        <button
          className="dropdown-trigger deck-tree-trigger"
          type="button"
          ref={triggerRef}
          disabled={disabled}
          aria-haspopup="tree"
          aria-expanded={open}
          aria-controls="anki-deck-tree"
          onClick={() => {
            if (open) setOpen(false);
            else openTree();
          }}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown" || event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              openTree();
            }
          }}
        >
          <span className="dropdown-value">{ankiDeckDisplayName(value)}</span>
          <ChevronDown className="dropdown-chevron" size={16} />
        </button>
        {open && (
          <div className="deck-tree-menu">
            <div
              className="deck-tree-list"
              id="anki-deck-tree"
              role="tree"
              aria-label={label}
            >
              {visibleNodes.map((node, index) => {
                const current = node.name === value;
                return (
                  <div
                    className={`deck-tree-row ${current ? "selected" : ""}`}
                    key={node.name}
                    role="none"
                    style={{ "--deck-indent": `${(node.depth - 1) * 16}px` } as CSSProperties}
                  >
                    {node.hasChildren ? (
                      <button
                        className="deck-tree-toggle"
                        type="button"
                        tabIndex={-1}
                        aria-label={`${node.expanded ? "收起" : "展开"} ${node.label}`}
                        onClick={() => toggleExpanded(node.name)}
                      >
                        <ChevronRight className={node.expanded ? "expanded" : ""} size={15} />
                      </button>
                    ) : (
                      <span className="deck-tree-leaf-space" aria-hidden="true" />
                    )}
                    <button
                      className={`deck-tree-item ${node.selectable ? "" : "group-only"}`}
                      type="button"
                      role="treeitem"
                      aria-level={node.depth}
                      aria-expanded={node.hasChildren ? node.expanded : undefined}
                      aria-selected={current}
                      tabIndex={node.name === activeName ? 0 : -1}
                      ref={(element) => {
                        if (element) itemRefs.current.set(node.name, element);
                        else itemRefs.current.delete(node.name);
                      }}
                      onClick={() => {
                        if (node.selectable) choose(node.name);
                        else toggleExpanded(node.name);
                      }}
                      onKeyDown={(event) => {
                        if (event.key === "ArrowDown") {
                          event.preventDefault();
                          setActiveName(visibleNodes[Math.min(index + 1, visibleNodes.length - 1)].name);
                        } else if (event.key === "ArrowUp") {
                          event.preventDefault();
                          setActiveName(visibleNodes[Math.max(index - 1, 0)].name);
                        } else if (event.key === "Home") {
                          event.preventDefault();
                          setActiveName(visibleNodes[0].name);
                        } else if (event.key === "End") {
                          event.preventDefault();
                          setActiveName(visibleNodes[visibleNodes.length - 1].name);
                        } else if (event.key === "ArrowRight" && node.hasChildren) {
                          event.preventDefault();
                          if (!node.expanded) toggleExpanded(node.name);
                          else if (visibleNodes[index + 1]) setActiveName(visibleNodes[index + 1].name);
                        } else if (event.key === "ArrowLeft") {
                          event.preventDefault();
                          if (node.expanded) toggleExpanded(node.name);
                          else {
                            const parent = ankiDeckParent(node.name);
                            if (parent) setActiveName(parent);
                          }
                        } else if (event.key === "Enter" || event.key === " ") {
                          event.preventDefault();
                          if (node.selectable) choose(node.name);
                          else toggleExpanded(node.name);
                        }
                      }}
                    >
                      <span>{node.label}</span>
                      {current && <Check size={15} />}
                    </button>
                  </div>
                );
              })}
            </div>
          </div>
        )}
      </div>
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

function DeviceGroup({ icon, title, note, devices, devicesReady, selectedDeviceId, specialRows, disabled, onSelectDevice }: {
  icon: React.ReactNode;
  title: string;
  note?: string;
  devices: AudioDevice[];
  devicesReady: boolean;
  selectedDeviceId: number | null;
  specialRows: Array<{
    key: string;
    name: string;
    description: string;
    chosen: boolean;
    onSelect: () => void;
  }>;
  disabled: boolean;
  onSelectDevice: (id: number) => void;
}) {
  return (
    <div className="device-group">
      <div className="device-group-title">
        <span className="device-group-icon">{icon}</span>
        <div><h3>{title}</h3>{note && <span>{note}</span>}</div>
      </div>
      <div className="device-list">
        {specialRows.map((row) => (
          <DeviceRow
            key={row.key}
            name={row.name}
            description={row.description}
            chosen={row.chosen}
            disabled={disabled}
            onSelect={row.onSelect}
          />
        ))}
        {devices.map((device) => (
          <DeviceRow
            key={device.id}
            name={device.name}
            description={`${device.sample_rate} Hz · ${device.channels} 声道${device.is_default ? " · 默认" : ""}`}
            chosen={selectedDeviceId === device.id}
            disabled={disabled}
            onSelect={() => onSelectDevice(device.id)}
          />
        ))}
        {!devicesReady
          ? <p className="device-empty">正在扫描设备…</p>
          : !devices.length && <p className="device-empty">未发现其他可用设备。</p>}
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
    <label className={`device-row ${chosen ? "chosen" : ""} ${disabled ? "disabled" : ""}`}>
      <input type="radio" aria-label={name} checked={chosen} disabled={disabled} onChange={onSelect} />
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
  const [ankiState, setAnkiState] = useState<AnkiAddState>("idle");
  const [ankiFeedback, setAnkiFeedback] = useState("");
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
    setAnkiState("idle");
    setAnkiFeedback("");
  }, [lookup.term, lookup.context]);

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
    if (!entry || ankiState === "adding") return;
    const cardContent = ankiDictionaryContent(visibleEntries);
    setAnkiState("adding");
    setAnkiFeedback("");
    if (demo) {
      setAnkiState("success");
      setAnkiFeedback(`已创建演示笔记 #42，写入 ${visibleEntries.length} 组释义`);
      return;
    }
    try {
      const result = await coreApi.createCard({
        term: lookup.term,
        reading: entry.reading,
        definition: cardContent.definition,
        context: lookup.context,
        dictionary: cardContent.dictionary,
        language: entry.language,
      });
      setAnkiState("success");
      setAnkiFeedback(`已创建 Anki 笔记 #${result.note_id}，写入 ${visibleEntries.length} 组释义`);
    } catch (reason) {
      setAnkiState("error");
      setAnkiFeedback(reason instanceof Error ? reason.message : "制卡失败，请重试");
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
      <button className={`anki-button anki-state-${ankiState}`} type="button" disabled={!entry || ankiState === "adding" || ankiState === "success"} onClick={() => void add()}>
        {ankiState === "success"
          ? <Check size={16} />
          : ankiState === "error"
            ? <TriangleAlert size={16} />
            : <PlusCircle size={16} />}
        {ankiButtonLabel(ankiState)}
      </button>
      {ankiFeedback && (
        <p className={`dictionary-anki-feedback ${ankiState}`} role={ankiState === "error" ? "alert" : "status"}>
          {ankiFeedback}
        </p>
      )}
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
