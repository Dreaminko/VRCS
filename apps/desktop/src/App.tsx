import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, RefObject } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
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
  FolderOpen,
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
import { ApiError } from "./api-error";
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
import { open } from "@tauri-apps/plugin-dialog";
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
import { shouldFollowLiveScroll } from "./live-scroll";
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
import {
  changeUiLanguage,
  currentUiLanguagePreference,
} from "./i18n";
import type { UiLanguagePreference } from "./ui-language";

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

const NATIVE_APP = isTauri();
const CONVERSATION_STARTS_KEY = "vrcs.conversation-starts.v1";
const SIDEBAR_OPEN_KEY = "vrcs.conversation-sidebar-open";



const MODEL_PRESENTATION: Record<AsrModelRecord["id"], {
  name: string;
  descriptionKey: string;
}> = {
  tiny: { name: "Tiny", descriptionKey: "settings.recognition.models.tiny" },
  base: { name: "Base", descriptionKey: "settings.recognition.models.base" },
  small: { name: "Small", descriptionKey: "settings.recognition.models.small" },
  medium: { name: "Medium", descriptionKey: "settings.recognition.models.medium" },
  "large-v3": { name: "Large v3", descriptionKey: "settings.recognition.models.largeV3" },
};

function formatBytes(bytes: number, locale: string): string {
  if (bytes < 1_000_000) {
    return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 0 }).format(Math.max(0, bytes / 1_000))} KB`;
  }
  if (bytes < 1_000_000_000) {
    return `${new Intl.NumberFormat(locale, {
      maximumFractionDigits: bytes < 100_000_000 ? 1 : 0,
    }).format(bytes / 1_000_000)} MB`;
  }
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(bytes / 1_000_000_000)} GB`;
}

function timestamp(value: string, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

function localizedError(
  reason: unknown,
  t: TFunction,
  fallbackKey: string,
): string {
  if (reason instanceof ApiError) {
    const fallback = t(fallbackKey);
    return t(`errors.${reason.code}`, {
      ...reason.params,
      defaultValue: fallback,
    });
  }
  if (
    reason
    && typeof reason === "object"
    && "code" in reason
    && typeof reason.code === "string"
  ) {
    return t(`errors.${reason.code}`, { defaultValue: t(fallbackKey) });
  }
  return reason instanceof Error ? reason.message : t(fallbackKey);
}

function storedConversationStarts() {
  try {
    const value = JSON.parse(localStorage.getItem(CONVERSATION_STARTS_KEY) ?? "[]") as unknown;
    return Array.isArray(value) ? value.filter((item): item is number => typeof item === "number" && Number.isFinite(item)).slice(-50) : [];
  } catch {
    return [];
  }
}

function conversationTime(
  value: string,
  locale: string,
  todayLabel: string,
  yesterdayLabel: string,
) {
  const date = new Date(value);
  const today = new Date();
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  const sameDay = (left: Date, right: Date) => left.toDateString() === right.toDateString();
  if (sameDay(date, today)) return `${todayLabel} ${timestamp(value, locale)}`;
  if (sameDay(date, yesterday)) return `${yesterdayLabel} ${timestamp(value, locale)}`;
  return new Intl.DateTimeFormat(locale, { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" }).format(date);
}

function useDismissibleLayer(
  open: boolean,
  rootRef: RefObject<HTMLElement | null>,
  onClose: () => void,
) {
  const onCloseRef = useRef(onClose);
  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    if (!open) return;
    const closeOnOutside = (event: PointerEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) onCloseRef.current();
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onCloseRef.current();
      }
    };
    document.addEventListener("pointerdown", closeOnOutside);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutside);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [open, rootRef]);
}

function App() {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const openedAt = useRef(Date.now()).current;
  const [page, setPage] = useState<Page>("live");
  const [connection, setConnection] = useState<ConnectionState>("connecting");
  const [coreConfigured, setCoreConfigured] = useState(false);
  const [health, setHealth] = useState<Health | null>(null);
  const [subtitles, setSubtitles] = useState<Subtitle[]>([]);
  const [settings, setSettings] = useState<Settings | null>(null);
  const persistedSettingsRef = useRef<Settings | null>(null);
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [devicesReady, setDevicesReady] = useState(false);
  const [asrCapabilities, setAsrCapabilities] = useState<AsrCapabilities | null>(null);
  const [dictionarySources, setDictionarySources] = useState<DictionarySource[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [vrchatWarningOpen, setVrchatWarningOpen] = useState(false);
  const [cudaRuntimeWarningOpen, setCudaRuntimeWarningOpen] = useState(false);
  const cudaRuntimeWarningShownRef = useRef(false);
  const [lookup, setLookup] = useState<Lookup | null>(null);
  const [compact, setCompact] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(() => localStorage.getItem(SIDEBAR_OPEN_KEY) !== "false");
  const [conversationStarts, setConversationStarts] = useState(storedConversationStarts);
  const [selectedConversationId, setSelectedConversationId] = useState<string | null>(null);
  const conversations = useMemo(
    () => groupConversations(subtitles, conversationStarts, openedAt, {
      untitled: t("conversations.untitled"),
      newConversation: t("conversations.new"),
    }),
    [conversationStarts, i18n.resolvedLanguage, openedAt, subtitles, t],
  );
  const activeConversation = conversations[0];
  const selectedConversation = conversations.find((conversation) => conversation.id === selectedConversationId) ?? activeConversation;
  const liveScrollRef = useRef<HTMLDivElement>(null);
  const previousLiveScrollTopRef = useRef(0);
  const [followingLiveSubtitles, setFollowingLiveSubtitles] = useState(true);
  const showingActiveConversation = selectedConversation?.id === activeConversation?.id;
  const liveAutoScrollActive = page === "live"
    && (health?.capture_running ?? false)
    && showingActiveConversation;

  const scrollLiveViewToBottom = useCallback((behavior: ScrollBehavior = "smooth") => {
    const scrollRegion = liveScrollRef.current;
    if (!scrollRegion) return;
    setFollowingLiveSubtitles(true);
    previousLiveScrollTopRef.current = scrollRegion.scrollTop;
    scrollRegion.scrollTo({ top: scrollRegion.scrollHeight, behavior });
  }, []);

  useEffect(() => {
    if (page !== "live") return;
    setFollowingLiveSubtitles(true);
    const frame = window.requestAnimationFrame(() => scrollLiveViewToBottom("auto"));
    return () => window.cancelAnimationFrame(frame);
  }, [page, scrollLiveViewToBottom, selectedConversation?.id]);

  useEffect(() => {
    if (!liveAutoScrollActive || !followingLiveSubtitles) return;
    const frame = window.requestAnimationFrame(() => scrollLiveViewToBottom());
    return () => window.cancelAnimationFrame(frame);
  }, [
    followingLiveSubtitles,
    liveAutoScrollActive,
    scrollLiveViewToBottom,
    selectedConversation?.updatedAt,
  ]);

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
    let cancelled = false;
    void initializeCoreApi()
      .then(() => {
        if (!cancelled) setCoreConfigured(true);
      })
      .catch((reason) => {
        if (!cancelled) {
          setConnection("disconnected");
          setError(localizedError(reason, t, "errors.core.initialize"));
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const refresh = useCallback(async () => {
    if (!coreConfigured) return;
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
      setError(localizedError(reason, t, "errors.core.connect"));
    }
  }, [coreConfigured]);

  useEffect(() => {
    if (!coreConfigured) return;
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
    if (!coreConfigured) return;
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
    if (!coreConfigured) return;
    try {
      setDevices(await coreApi.devices());
      setDevicesReady(true);
      setError(null);
    } catch (reason) {
      setDevicesReady(false);
      setError(localizedError(reason, t, "errors.audio.devices"));
    }
  }, [coreConfigured]);

  const loadDictionaries = useCallback(async () => {
    if (!coreConfigured) return;
    try {
      setDictionarySources(await coreApi.dictionaries());
      setError(null);
    } catch (reason) {
      setError(localizedError(reason, t, "errors.dictionary.list"));
    }
  }, [coreConfigured]);

  const loadAsrCapabilities = useCallback(async () => {
    if (!coreConfigured) return;
    try {
      setAsrCapabilities(await coreApi.asrCapabilities());
      setError(null);
    } catch (reason) {
      setError(localizedError(reason, t, "errors.asr.capabilities"));
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
    try {
      if (health?.capture_running) await coreApi.stop();
      else await coreApi.start();
      setHealth(await coreApi.health());
      setError(null);
    } catch (reason) {
      const message = localizedError(reason, t, "errors.operation");
      if (shouldShowVrchatNotRunningWarning(
        reason,
        settings?.audio.output.mode === "vrchat",
      )) {
        setError(null);
        setLookup(null);
        setVrchatWarningOpen(true);
        if (compact) {
          try {
            await resizeCompactWindow(true);
          } catch (resizeError) {
            setError(localizedError(resizeError, t, "errors.window.warningExpand"));
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
      Boolean(health?.capture_running)
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
      saved = await coreApi.saveSettings(next);
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
          const applyMessage = localizedError(reason, t, "errors.settings.apply");
          const recoveryMessage = localizedError(recoveryError, t, "errors.unknown");
          throw new Error(
            t("errors.settings.recovery", {
              applyMessage,
              recoveryMessage,
            }),
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
      onCommit: (_saved) => {
        setError(null);
        void loadAsrCapabilitiesRef.current();
      },
      onError: (reason) => {
        if (persistedSettingsRef.current) setSettings(persistedSettingsRef.current);
        setError(localizedError(reason, t, "errors.settings.apply"));
      },
    });
  }
  const saveSettings = settingsAutosaveRef.current;

  const importDictionary = async (file: File) => {
    const imported = await coreApi.importDictionary(file);
    await loadDictionaries();
    return imported;
  };

  const deleteDictionary = async (id: number) => {
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
      const entries = await coreApi.lookup(term);
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
      setError(localizedError(reason, t, "errors.dictionary.lookup"));
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
      setError(localizedError(reason, t, "errors.window.compactCollapse"));
    });
  };

  const closeVrchatWarning = () => {
    setVrchatWarningOpen(false);
    if (compact) {
      void resizeCompactWindow(false).catch((reason) => {
        setError(localizedError(reason, t, "errors.window.compactCollapse"));
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
        throw new Error(t("errors.window.alwaysOnTop"));
      }
      setCompact(next);
      setError(null);
    } catch (reason) {
      setError(localizedError(reason, t, "errors.window.compactToggle"));
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
        {lookup && <DictionaryPopover lookup={lookup} compact onClose={closeCompactLookup} />}
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
        {page === "live" && sidebarOpen && <button className="sidebar-scrim" type="button" aria-label={t("conversations.closeSidebar")} onClick={() => setSidebarOpen(false)} />}
        <div
          className="app-scroll-region"
          ref={liveScrollRef}
          onScroll={(event) => {
            if (page !== "live") return;
            const scrollRegion = event.currentTarget;
            setFollowingLiveSubtitles((current) => shouldFollowLiveScroll(current, {
              scrollTop: scrollRegion.scrollTop,
              previousScrollTop: previousLiveScrollTopRef.current,
              scrollHeight: scrollRegion.scrollHeight,
              clientHeight: scrollRegion.clientHeight,
            }));
            previousLiveScrollTopRef.current = scrollRegion.scrollTop;
          }}
        >
          <main className={`workspace workspace-${page}`}>
          {page === "live" && <TopStatus connection={connection} health={health} settings={settings} />}

          {error && (
            <div className="error-banner" role="alert">
              <span>{error}</span>
              <button type="button" aria-label={t("common.closeError")} onClick={() => setError(null)}><X size={18} /></button>
            </div>
          )}

          {page === "live" && (
            <>
              {selectedConversation && activeConversation && selectedConversation.id !== activeConversation.id && (
                <div className="conversation-history-notice">
                  <Clock3 size={15} />
                  <span>{t("conversations.viewingPast", {
                    time: conversationTime(
                      selectedConversation.startedAt,
                      locale,
                      t("date.today"),
                      t("date.yesterday"),
                    ),
                  })}</span>
                  <button type="button" onClick={() => setSelectedConversationId(activeConversation.id)}>{t("conversations.returnCurrent")}</button>
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

      {page === "live" && !followingLiveSubtitles && (
        <button
          className="live-scroll-to-bottom"
          type="button"
          aria-label={t("live.returnToBottom")}
          title={t("live.returnToBottomShort")}
          onClick={() => scrollLiveViewToBottom()}
        >
          <ChevronDown size={20} strokeWidth={2} />
        </button>
      )}

      <BottomDock
        page={page}
        running={health?.capture_running ?? false}
        onPageChange={(next) => { setLookup(null); setPage(next); }}
        onCompact={() => void toggleCompact()}
        onCapture={() => void toggleCapture()}
      />

      {lookup && <DictionaryPopover lookup={lookup} onClose={() => setLookup(null)} />}
      {vrchatWarningOpen && <VrchatNotRunningDialog onClose={closeVrchatWarning} />}
      {cudaRuntimeWarningOpen && <CudaRuntimeDialog onClose={() => setCudaRuntimeWarningOpen(false)} />}
    </div>
  );
}

function VrchatNotRunningDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  return (
    <WarningDialog
      id="vrchat-warning"
      title={t("warnings.vrchat.title")}
      description={t("warnings.vrchat.description")}
      onClose={onClose}
    />
  );
}

function CudaRuntimeDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  return (
    <WarningDialog
      id="cuda-runtime-warning"
      title={t("warnings.cuda.title")}
      description={t("warnings.cuda.description")}
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
  const { t } = useTranslation();
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
        <button ref={confirmRef} className="primary-button" type="button" onClick={onClose}>{t("common.understood")}</button>
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
  const { t } = useTranslation();
  const active = conversations.find((conversation) => conversation.id === activeId);
  const history = conversations.filter((conversation) => conversation.id !== activeId);

  if (!open) {
    return (
      <aside className="conversation-sidebar conversation-sidebar-collapsed" aria-label={t("conversations.sidebar")}>
        <button className="sidebar-icon-button" type="button" aria-label={t("conversations.expandSidebar")} aria-expanded="false" onClick={onToggle}><PanelLeftOpen size={19} /></button>
        <button className="sidebar-icon-button sidebar-new-icon" type="button" aria-label={t("conversations.create")} onClick={onNew}><Plus size={20} /></button>
        {active && <button className={`sidebar-icon-button sidebar-current-icon ${selectedId === active.id ? "active" : ""}`} type="button" aria-label={t("conversations.viewCurrent")} onClick={() => onSelect(active.id)}><MessageSquareText size={19} /></button>}
      </aside>
    );
  }

  return (
    <aside className="conversation-sidebar" aria-label={t("conversations.sidebar")}>
      <div className="conversation-sidebar-header">
        <span>{t("conversations.title")}</span>
        <button className="sidebar-icon-button" type="button" aria-label={t("conversations.collapseSidebar")} aria-expanded="true" onClick={onToggle}><PanelLeftClose size={19} /></button>
      </div>
      <button className="new-conversation-button" type="button" onClick={onNew}><Plus size={18} />{t("conversations.create")}</button>
      <div className="conversation-sidebar-list">
        {active && (
          <section className="conversation-group" aria-labelledby="current-conversation-heading">
            <h2 id="current-conversation-heading">{t("conversations.current")}</h2>
            <ConversationButton conversation={active} active selected={selectedId === active.id} onSelect={onSelect} />
          </section>
        )}
        <section className="conversation-group" aria-labelledby="recent-conversations-heading">
          <h2 id="recent-conversations-heading">{t("conversations.previous")}</h2>
          {history.length ? history.map((conversation) => (
            <ConversationButton key={conversation.id} conversation={conversation} selected={selectedId === conversation.id} onSelect={onSelect} />
          )) : <p className="conversation-list-empty">{t("conversations.empty")}</p>}
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
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  return (
    <button
      className={`conversation-button ${selected ? "selected" : ""}`}
      type="button"
      aria-current={selected ? "true" : undefined}
      onClick={() => onSelect(conversation.id)}
    >
      <span className="conversation-button-title"><MessageSquareText size={16} /><strong>{conversation.title}</strong>{active && <i aria-label={t("conversations.current")} />}</span>
      <span className="conversation-button-meta">
        <time>{conversationTime(conversation.startedAt, locale, t("date.today"), t("date.yesterday"))}</time>
        <span>{t("conversations.subtitleCount", { count: conversation.subtitles.length })}</span>
      </span>
    </button>
  );
}

function WindowChrome() {
  const { t } = useTranslation();
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
    <header className="window-chrome" data-tauri-drag-region aria-label={t("window.controls")}>
      <div className="window-drag-region" data-tauri-drag-region />
      <div className="window-actions">
        <button type="button" aria-label={t("window.minimize")} title={t("window.minimizeShort")} onClick={() => void runWindowAction("minimize")}><Minus size={15} strokeWidth={1.8} /></button>
        <button type="button" aria-label={t("window.maximize")} title={t("window.maximizeShort")} onClick={() => void runWindowAction("maximize")}><Square size={12} strokeWidth={1.7} /></button>
        <button className="window-close" type="button" aria-label={t("window.close")} title={t("common.close")} onClick={() => void runWindowAction("close")}><X size={15} strokeWidth={1.8} /></button>
      </div>
    </header>
  );
}

function TopStatus({ connection, health, settings }: {
  connection: ConnectionState;
  health: Health | null;
  settings: Settings | null;
}) {
  const { t } = useTranslation();
  const connectionLabel = t(`status.connection.${connection}`);
  return (
    <div className="top-status-row">
      <div className="status-summary" aria-label={t("status.summary")}>
        <div className={`core-summary connection-${connection}`}><span>Core</span><strong><i aria-hidden="true" />{connectionLabel}</strong></div>
        <i aria-hidden="true" />
        <div><span>{t("status.label")}</span><strong>{health?.capture_running ? t("status.transcribing") : t("status.waiting")}</strong></div>
        <i aria-hidden="true" />
        <div><span>{t("status.engine")}</span><strong>Whisper {capitalize(settings?.asr.model ?? "small")}</strong></div>
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
  const { t } = useTranslation();
  const chronological = [...subtitles].reverse();
  return (
    <section className="conversation" aria-label={t("live.title")}>
      {chronological.length ? chronological.map((subtitle, index) => (
        <ChatBubble key={subtitle.id ?? `${subtitle.created_at}-${index}`} subtitle={subtitle} onSelect={onSelect} />
      )) : (
        <div className="empty-state"><MessageSquare size={22} /><p>{running ? t("live.listening") : t("live.startHint")}</p></div>
      )}
      {running && (
        <div className="message-group source-speaker streaming-message">
          <div className="bubble">{t("live.transcribing")}<span className="streaming-ellipsis" aria-hidden="true">…</span></div>
        </div>
      )}
    </section>
  );
}

function ChatBubble({ subtitle, onSelect }: { subtitle: Subtitle; onSelect: (context: string) => Promise<void> }) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const source: SubtitleSource = subtitle.source ?? "speaker";
  const mine = source === "microphone";
  return (
    <article className={`message-group source-${source}`}>
      <div className="message-meta">
        {!mine && <Volume2 size={14} />}
        {mine && <time>{timestamp(subtitle.created_at, locale)}</time>}
        <span>{mine ? t("live.microphoneMe") : t("live.speakerOther")}</span>
        {!mine && <time>{timestamp(subtitle.created_at, locale)}</time>}
        {mine && <Mic size={14} />}
      </div>
      <p className="bubble" onMouseUp={() => void onSelect(subtitle.text)}>{subtitle.text}</p>
    </article>
  );
}

function HistoryView({ subtitles, onSelect }: { subtitles: Subtitle[]; onSelect: (context: string) => Promise<void> }) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
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
        <div><h2>{t("history.title")}</h2><span>{t("history.recordCount", { count: filtered.length })}</span></div>
        <div className="history-filters">
          <DropdownField
            compact
            icon={<Languages size={15} />}
            label={t("history.language")}
            value={language}
            options={[
              { value: "all", label: t("languages.all") },
              { value: "ja", label: t("languages.japanese") },
              { value: "en", label: t("languages.english") },
              { value: "zh", label: t("languages.chinese") },
              { value: "ko", label: t("languages.korean") },
            ]}
            onChange={setLanguage}
          />
          <DropdownField
            compact
            icon={<CalendarDays size={15} />}
            label={t("history.dateRange")}
            value={range}
            options={[
              { value: "all", label: t("history.allTime") },
              { value: "today", label: t("date.today") },
              { value: "week", label: t("history.lastSevenDays") },
            ]}
            onChange={setRange}
          />
        </div>
      </div>
      {filtered.length ? (
        <div className="history-list">{filtered.map((subtitle, index) => (
          <article key={subtitle.id ?? `${subtitle.created_at}-${index}`} onMouseUp={() => void onSelect(subtitle.text)}>
            <time>{timestamp(subtitle.created_at, locale)}</time>
            <p>{subtitle.text}</p>
            <span>{subtitle.language?.toUpperCase() ?? "—"}</span>
          </article>
        ))}</div>
      ) : <div className="empty-state"><History size={22} /><p>{t("history.empty")}</p></div>}
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
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const [draft, setDraft] = useState(settings);
  const [activeCategory, setActiveCategory] = useState<SettingsCategory>("system");
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const [saveMessage, setSaveMessage] = useState("");
  const [desktopPreferences, setDesktopPreferences] = useState(defaultDesktopPreferences);
  const [desktopPreferencesReady, setDesktopPreferencesReady] = useState(false);
  const [desktopSaveState, setDesktopSaveState] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const [desktopMessage, setDesktopMessage] = useState("");
  const [uiLanguagePreference, setUiLanguagePreference] = useState<UiLanguagePreference>(
    currentUiLanguagePreference,
  );
  const [dictionaryBusy, setDictionaryBusy] = useState(false);
  const [dictionaryMessage, setDictionaryMessage] = useState("");
  const [managedModels, setManagedModels] = useState<AsrModelRecord[]>([]);
  const [modelsReady, setModelsReady] = useState(false);
  const [modelMessage, setModelMessage] = useState("");
  const [modelDirectoryText, setModelDirectoryText] = useState(settings.storage.model_directory);
  const [ankiStatus, setAnkiStatus] = useState<AnkiStatus | null>(null);
  const [ankiBusy, setAnkiBusy] = useState(false);
  const [ankiMessage, setAnkiMessage] = useState("");
  const [ankiPortText, setAnkiPortText] = useState(String(settings.anki.port));
  const [ankiPortError, setAnkiPortError] = useState("");
  const dictionaryFileRef = useRef<HTMLInputElement>(null);
  const draftRef = useRef(settings);
  const saveVersionRef = useRef(0);
  const savingRef = useRef(false);
  const managedModelsRef = useRef(managedModels);
  managedModelsRef.current = managedModels;
  useEffect(() => {
    draftRef.current = settings;
    if (savingRef.current) return;
    setDraft(settings);
    setModelDirectoryText(settings.storage.model_directory);
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
        setDesktopMessage(localizedError(reason, t, "errors.desktop.read"));
        setDesktopSaveState("error");
        setDesktopPreferencesReady(true);
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);
  const loadModels = useCallback(async () => {
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
      setModelMessage(localizedError(reason, t, "errors.asr.models"));
    }
  }, [onModelsChanged]);
  useEffect(() => {
    void loadModels();
  }, [loadModels]);
  useEffect(() => {
    if (activeCategory !== "recognition") return;
    const timer = window.setInterval(() => void loadModels(), 750);
    return () => window.clearInterval(timer);
  }, [activeCategory, loadModels]);
  const loadAnkiStatus = useCallback(async () => {
    setAnkiBusy(true);
    setAnkiMessage("");
    try {
      const next = await coreApi.ankiStatus();
      setAnkiStatus(next);
      setAnkiMessage(t(`apiStatus.${next.status_code}`, {
        ...next.params,
        defaultValue: next.detail,
      }));
    } catch (reason) {
      setAnkiStatus(null);
      setAnkiMessage(localizedError(reason, t, "errors.anki.status"));
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
    savingRef.current = true;
    draftRef.current = next;
    setDraft(next);
    setSaveState("saving");
    setSaveMessage("");
    void onSave(next).then(
      (saved) => {
        if (version !== saveVersionRef.current) return;
        savingRef.current = false;
        draftRef.current = saved;
        setDraft(saved);
        setSaveState("saved");
        afterSave?.();
      },
      (reason) => {
        if (version !== saveVersionRef.current) return;
        savingRef.current = false;
        setSaveMessage(localizedError(reason, t, "errors.settings.apply"));
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
      setAnkiPortError(t("settings.anki.invalidPort"));
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
  const updateVad = <K extends keyof Settings["vad"]>(key: K, value: Settings["vad"][K]) => {
    applySettings((current) => ({
      ...current,
      vad: { ...current.vad, [key]: value },
    }));
  };
  const updateModelDirectory = (value: string) => {
    const directory = value.trim();
    if (!directory) {
      setSaveMessage(t("settings.recognition.modelDirectoryRequired"));
      setSaveState("error");
      return;
    }
    setModelDirectoryText(directory);
    if (directory === draftRef.current.storage.model_directory) return;
    applySettings(
      (current) => ({
        ...current,
        storage: { ...current.storage, model_directory: directory },
      }),
      () => {
        void loadModels();
        void onModelsChanged();
      },
    );
  };
  const chooseModelDirectory = async () => {
    try {
      const directory = await open({
        directory: true,
        multiple: false,
        title: t("settings.recognition.chooseModelDirectory"),
      });
      if (typeof directory === "string") updateModelDirectory(directory);
    } catch (reason) {
      setSaveMessage(localizedError(reason, t, "errors.dialog.folder"));
      setSaveState("error");
    }
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
      setDesktopMessage(localizedError(reason, t, "errors.desktop.save"));
      setDesktopSaveState("error");
    }
  };
  const updateUiLanguage = async (preference: UiLanguagePreference) => {
    const previous = uiLanguagePreference;
    setUiLanguagePreference(preference);
    setDesktopSaveState("saving");
    setDesktopMessage("");
    try {
      await changeUiLanguage(preference);
      setDesktopSaveState("saved");
    } catch (reason) {
      setUiLanguagePreference(previous);
      setDesktopMessage(localizedError(reason, t, "errors.desktop.language"));
      setDesktopSaveState("error");
    }
  };
  const outputDevices = devices.filter((device) => device.is_loopback);
  const microphoneDevices = devices.filter((device) => !device.is_loopback);
  const deviceErrors = devicesReady
    ? audioSelectionErrors(draft, devices, (key) => t(key))
    : [];
  const asrError = asrSelectionError(draft, asrCapabilities, (key) => t(key));
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
    ? t("settings.recognition.modelStatus.notDownloaded")
    : selectedModelStatus === "loading"
      ? t("settings.recognition.modelStatus.loading")
      : selectedModelStatus === "error"
        ? t("settings.recognition.modelStatus.error")
        : selectedModelStatus
          ? t("settings.recognition.modelStatus.ready")
          : t("settings.recognition.modelStatus.checking");
  const installedModels = managedModels.filter((model) =>
    ["downloaded", "loading", "ready"].includes(model.status),
  );
  const downloadingModels = managedModels.filter((model) => model.status === "downloading");
  const selectableModels = modelsReady
    ? managedModels.filter((model) =>
        model.id === draft.asr.model
        || ["downloaded", "loading", "ready"].includes(model.status),
      )
    : (asrCapabilities?.models ?? []).filter((model) =>
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
    { id: "system", label: t("settings.categories.system"), icon: <SlidersHorizontal size={18} /> },
    { id: "audio", label: t("settings.categories.audio"), icon: <Volume2 size={18} /> },
    { id: "recognition", label: t("settings.categories.recognition"), icon: <Languages size={18} /> },
    { id: "dictionary", label: t("settings.categories.dictionary"), icon: <BookOpen size={18} /> },
    { id: "anki", label: "Anki", icon: <PlusCircle size={18} /> },
    { id: "debug", label: "Debug", icon: <Wrench size={18} /> },
  ];
  const debugRows = [
    { label: t("settings.debug.schema"), value: `v${draft.schema_version}` },
    { label: t("settings.debug.coreAddress"), value: `${draft.server.host}:${draft.server.port}` },
    { label: t("settings.debug.databasePath"), value: draft.storage.database_path },
    { label: t("settings.debug.modelDirectory"), value: draft.storage.model_directory },
    { label: t("settings.debug.sampleRate"), value: `${new Intl.NumberFormat(locale).format(draft.audio.sample_rate)} Hz` },
    { label: t("settings.debug.silence"), value: t("units.seconds", { value: draft.vad.silence_seconds.toFixed(1) }) },
    { label: t("settings.debug.maxSegment"), value: t("units.seconds", { value: draft.vad.max_speech_seconds }) },
    { label: t("settings.debug.historyLimit"), value: t("settings.debug.subtitleLimitValue", { count: draft.storage.subtitle_history_limit, formatted: new Intl.NumberFormat(locale).format(draft.storage.subtitle_history_limit) }) },
    { label: t("settings.debug.modelStatus"), value: modelStatus },
    { label: t("settings.debug.cuda"), value: asrCapabilities?.cuda.available ? t("settings.debug.availableDevices", { count: asrCapabilities.cuda.device_count }) : t("common.unavailable") },
    { label: t("settings.debug.transcription"), value: disabled ? t("status.transcribing") : t("status.stopped") },
    { label: t("settings.debug.audioDevices"), value: t("settings.debug.audioDeviceCounts", { outputs: outputDevices.length, microphones: microphoneDevices.length }) },
    { label: t("settings.debug.dictionaries"), value: dictionaries.length ? t("settings.dictionary.count", { count: dictionaries.length }) : t("settings.dictionary.noneImported") },
  ];
  const settingsActionText = activeCategory === "dictionary"
    ? t("settings.action.dictionaryImmediate")
    : activeCategory === "anki"
      ? ankiPortError
        || (saveState === "saving"
          ? t("settings.action.savingAnki")
          : saveState === "error"
            ? saveMessage || t("settings.action.ankiSaveFailed")
            : ankiMessage || t("settings.action.ankiAutoCheck"))
    : activeCategory === "system"
      ? !desktopPreferencesReady
        ? t("settings.action.readingDesktop")
        : desktopSaveState === "saving"
          ? t("settings.action.savingDesktop")
          : desktopSaveState === "saved"
            ? t("settings.action.desktopSaved")
            : desktopSaveState === "error"
              ? desktopMessage || t("settings.action.desktopSaveFailed")
              : t("settings.action.desktopHint")
    : activeCategory === "debug"
      ? t("settings.action.debugHint")
      : validationError
          ? validationError
          : saveState === "saving"
        ? t("settings.action.applying")
        : saveState === "saved"
          ? t("settings.action.applied")
          : saveState === "error"
            ? saveMessage || t("settings.action.applyFailed")
            : t("settings.action.immediate");
  const visibleSaveState = activeCategory === "system"
    ? desktopSaveState
    : activeCategory === "anki" && (ankiPortError || (ankiStatus && !ankiStatus.configuration_valid))
      ? "error"
      : saveState;
  const chooseDictionary = async (file?: File) => {
    if (!file) return;
    setDictionaryBusy(true);
    setDictionaryMessage(t("settings.dictionary.importing", { file: file.name }));
    try {
      const imported = await onImportDictionary(file);
      setDictionaryMessage(t("settings.dictionary.imported", {
        title: imported.title,
        count: imported.entry_count,
        formatted: new Intl.NumberFormat(locale).format(imported.entry_count),
      }));
    } catch (reason) {
      setDictionaryMessage(localizedError(reason, t, "errors.dictionary.import"));
    } finally {
      setDictionaryBusy(false);
      if (dictionaryFileRef.current) dictionaryFileRef.current.value = "";
    }
  };
  const removeDictionary = async (dictionary: DictionarySource) => {
    if (!window.confirm(t("settings.dictionary.confirmRemove", { title: dictionary.title }))) return;
    setDictionaryBusy(true);
    try {
      await onDeleteDictionary(dictionary.id);
      setDictionaryMessage(t("settings.dictionary.removed", { title: dictionary.title }));
    } catch (reason) {
      setDictionaryMessage(localizedError(reason, t, "errors.dictionary.remove"));
    } finally {
      setDictionaryBusy(false);
    }
  };
  const downloadModel = async (model: AsrModelRecord) => {
    setModelMessage(t("settings.recognition.preparingDownload", { name: MODEL_PRESENTATION[model.id].name }));
    try {
      await coreApi.downloadAsrModel(model.id);
      setModelMessage(t("settings.recognition.downloadQueued", { name: MODEL_PRESENTATION[model.id].name }));
      await loadModels();
    } catch (reason) {
      setModelMessage(localizedError(reason, t, "errors.asr.download"));
    }
  };
  const removeModel = async (model: AsrModelRecord) => {
    const name = MODEL_PRESENTATION[model.id].name;
    if (!window.confirm(t("settings.recognition.confirmDelete", { name }))) return;
    setModelMessage(t("settings.recognition.deleting", { name }));
    try {
      await coreApi.deleteAsrModel(model.id);
      await loadModels();
      await onModelsChanged();
      setModelMessage(t("settings.recognition.deleted", { name }));
    } catch (reason) {
      setModelMessage(localizedError(reason, t, "errors.asr.delete"));
    }
  };

  return (
    <section className="settings-surface">
      <div className="settings-tabbar-wrap">
        <div className="settings-tabbar" role="tablist" aria-label={t("settings.categories.label")}>
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
            <div><SlidersHorizontal size={18} /><h2>{t("settings.system.title")}</h2><span>{t("settings.system.subtitle")}</span></div>
            <p>{t("settings.system.saveImmediately")}</p>
          </div>
          <div className="system-language-setting">
            <div>
              <strong>{t("settings.system.language")}</strong>
              <small>{t("settings.system.languageDescription")}</small>
            </div>
            <Select
              label={t("settings.system.language")}
              value={uiLanguagePreference}
              options={[
                { value: "system", label: t("settings.system.followSystem") },
                { value: "zh-CN", label: "简体中文" },
                { value: "ja-JP", label: "日本語" },
                { value: "en-US", label: "English" },
              ]}
              disabled={desktopSaveState === "saving"}
              onChange={(value) => void updateUiLanguage(value as UiLanguagePreference)}
            />
          </div>
          <div className="settings-toggle-list">
            <PreferenceToggle
              title={t("settings.system.launchAtStartup")}
              description={t("settings.system.launchAtStartupDescription")}
              checked={desktopPreferences.launchAtStartup}
              disabled={!desktopPreferencesReady || desktopSaveState === "saving"}
              onChange={(enabled) => void updateDesktop("launchAtStartup", enabled)}
            />
            <PreferenceToggle
              title={t("settings.system.minimizeToTray")}
              description={t("settings.system.minimizeToTrayDescription")}
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
            <div><Languages size={18} /><h2>{t("settings.recognition.title")}</h2><span className="status-chip">{t("settings.recognition.status", { status: modelStatus })}</span></div>
            <p>{disabled ? t("settings.recognition.stopToModify") : t("settings.recognition.applyImmediately")}</p>
          </div>
          <div className={`recognition-runtime ${asrCapabilities?.cuda.available ? "available" : "unavailable"}`}>
            <span className="recognition-runtime-dot" aria-hidden="true" />
            <div>
              <strong>{t("settings.recognition.runtime")}</strong>
              <span>
                {asrCapabilities === null
                  ? t("settings.recognition.runtimeChecking")
                  : asrCapabilities.cuda.available
                    ? t("settings.recognition.cudaAvailable", { count: asrCapabilities.cuda.device_count })
                    : asrCapabilities.cuda.device_count > 0
                      ? t("settings.recognition.cudaRuntimeMissing")
                      : t("settings.recognition.cudaUnavailable")}
              </span>
            </div>
          </div>
          <div className="recognition-config">
            <div className="recognition-config-row">
              <div className="recognition-config-title">
                <Languages size={17} />
                <span><strong>{t("settings.recognition.content")}</strong><small>{t("settings.recognition.contentDescription")}</small></span>
              </div>
              <div className="recognition-config-fields">
                <Select
                  label={t("settings.recognition.model")}
                  helper={modelStatusLabel}
                  value={draft.asr.model}
                  options={selectableModels.map((model) => ({
                    value: model.id,
                    label: `${model.id} · ${
                      model.status === "not_downloaded"
                        ? t("settings.recognition.modelState.notDownloaded")
                        : model.status === "loading"
                          ? t("settings.recognition.modelState.loading")
                          : model.status === "error"
                            ? t("settings.recognition.modelState.error")
                            : t("settings.recognition.modelState.ready")
                    }`,
                  }))}
                  disabled={disabled}
                  onChange={(value) => updateAsr("model", value as Settings["asr"]["model"])}
                />
                <Select
                  label={t("settings.recognition.language")}
                  helper={t("settings.recognition.languageDescription")}
                  value={draft.asr.language}
                  options={[
                    { value: "auto", label: t("languages.auto") },
                    { value: "en", label: t("languages.english") },
                    { value: "ja", label: t("languages.japanese") },
                    { value: "zh", label: t("languages.chinese") },
                    { value: "ko", label: t("languages.korean") },
                    { value: "es", label: t("languages.spanish") },
                    { value: "fr", label: t("languages.french") },
                    { value: "de", label: t("languages.german") },
                  ]}
                  disabled={disabled}
                  onChange={(value) => updateAsr("language", value as Settings["asr"]["language"])}
                />
              </div>
            </div>
            <div className="recognition-config-row">
              <div className="recognition-config-title">
                <HardDrive size={17} />
                <span><strong>{t("settings.recognition.execution")}</strong><small>{t("settings.recognition.executionDescription")}</small></span>
              </div>
              <div className="recognition-config-fields">
                <Select
                  label={t("settings.recognition.device")}
                  helper={asrError ?? t("settings.recognition.deviceDescription")}
                  value={draft.asr.device}
                  options={[
                    { value: "auto", label: t("common.autoSelect") },
                    { value: "cpu", label: "CPU" },
                    ...(asrCapabilities?.cuda.available ? [{ value: "cuda", label: "CUDA" }] : []),
                    ...(draft.asr.device === "cuda" && !asrCapabilities?.cuda.available
                      ? [{ value: "cuda", label: `CUDA · ${t("common.unavailable")}` }]
                      : []),
                  ]}
                  disabled={disabled}
                  onChange={(value) => updateAsr("device", value as Settings["asr"]["device"])}
                />
                <Select
                  label={t("settings.recognition.computeType")}
                  helper={t("settings.recognition.computeTypeDescription")}
                  value={draft.asr.compute_type}
                  values={computeTypes}
                  disabled={disabled}
                  onChange={(value) => updateAsr("compute_type", value as Settings["asr"]["compute_type"])}
                />
              </div>
            </div>
            <div className="recognition-config-row">
              <div className="recognition-config-title">
                <Clock3 size={17} />
                <span><strong>{t("settings.recognition.segmentation")}</strong><small>{t("settings.recognition.segmentationDescription")}</small></span>
              </div>
              <div className="recognition-config-fields">
                <RangeField
                  label={t("settings.recognition.silence")}
                  helper={t("settings.recognition.silenceDescription")}
                  value={draft.vad.silence_seconds}
                  min={0.1}
                  max={2}
                  step={0.1}
                  disabled={disabled}
                  formatValue={(value) => t("units.seconds", { value: value.toFixed(1) })}
                  onCommit={(value) => updateVad("silence_seconds", value)}
                />
                <RangeField
                  label={t("settings.recognition.maxSegment")}
                  helper={t("settings.recognition.maxSegmentDescription")}
                  value={draft.vad.max_speech_seconds}
                  min={1}
                  max={30}
                  step={1}
                  disabled={disabled}
                  formatValue={(value) => t("units.seconds", { value })}
                  onCommit={(value) => updateVad("max_speech_seconds", value)}
                />
              </div>
            </div>
          </div>
          <section className="model-section recognition-models" aria-labelledby="local-models-heading">
          <div className="section-heading">
            <div>
              <HardDrive size={18} />
              <h2 id="local-models-heading">{t("settings.recognition.localModels")}</h2>
              <span>
                {downloadingModels.length
                  ? t("settings.recognition.downloadingCount", { count: downloadingModels.length })
                  : modelsReady
                    ? t("settings.recognition.installedCount", { count: installedModels.length })
                    : t("common.loading")}
              </span>
            </div>
            <button className="secondary-button" type="button" disabled={!modelsReady} onClick={() => void loadModels()}><RefreshCw size={15} />{t("common.refresh")}</button>
          </div>

          <div className="model-directory-setting">
            <label htmlFor="model-directory">
              <span>{t("settings.recognition.modelDirectory")}</span>
              <small>{t("settings.recognition.modelDirectoryDescription")}</small>
            </label>
            <div>
              <input
                id="model-directory"
                type="text"
                value={modelDirectoryText}
                disabled={disabled || downloadingModels.length > 0 || saveState === "saving"}
                spellCheck={false}
                onChange={(event) => setModelDirectoryText(event.target.value)}
                onBlur={() => updateModelDirectory(modelDirectoryText)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    event.currentTarget.blur();
                  }
                }}
              />
              <button
                className="secondary-button"
                type="button"
                disabled={!NATIVE_APP || disabled || downloadingModels.length > 0 || saveState === "saving"}
                title={NATIVE_APP ? t("settings.recognition.chooseFolder") : t("settings.recognition.browserPathHint")}
                onClick={() => void chooseModelDirectory()}
              >
                <FolderOpen size={16} />
                {t("settings.recognition.chooseFolder")}
              </button>
            </div>
          </div>

          {!modelsReady && managedModels.length === 0 ? (
            <div className="model-list-pending" role="status">
              <RefreshCw size={17} />
              <span>{t("settings.recognition.checkingLocalModels")}</span>
            </div>
          ) : (
            <div className="model-list">
              {managedModels.map((model) => {
                const presentation = MODEL_PRESENTATION[model.id];
                const downloaded = ["downloaded", "loading", "ready"].includes(model.status);
                const downloading = model.status === "downloading";
                const percentage = Math.round(model.progress * 100);
                const sizeLabel = downloading
                  ? `${formatBytes(model.downloaded_bytes, locale)} / ${formatBytes(model.total_bytes, locale)}`
                  : formatBytes(model.total_bytes, locale);
                return (
                  <article className={`model-row model-status-${model.status}`} key={model.id}>
                    <div className="model-row-body">
                      <div className="model-row-title">
                        <strong>{presentation.name}</strong>
                        {model.active && <span className="model-active-chip">{t("settings.recognition.inUse")}</span>}
                        <span className="model-size">{sizeLabel}</span>
                      </div>
                      <p>{t(presentation.descriptionKey)}</p>
                      <code>{model.repository}</code>
                      {downloading && (
                        <div className="model-progress-wrap">
                          <div
                            className="model-progress-track"
                            role="progressbar"
                            aria-label={t("settings.recognition.downloadProgress", { name: presentation.name })}
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
                        <span className="model-download-state"><RefreshCw size={15} />{t("common.downloading")}</span>
                      ) : downloaded ? (
                        model.active ? (
                          <span className="model-ready-state"><Check size={15} />{t("common.ready")}</span>
                        ) : (
                          <button className="model-delete-button" type="button" aria-label={t("settings.recognition.deleteModel", { name: presentation.name })} onClick={() => void removeModel(model)}><Trash2 size={16} /><span>{t("common.delete")}</span></button>
                        )
                      ) : (
                        <button className="model-download-button" type="button" onClick={() => void downloadModel(model)}><Download size={16} />{model.status === "error" ? t("common.retry") : t("common.download")}</button>
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
            <div><h2>{t("settings.audio.title")}</h2><span>{devices.length ? t("settings.audio.devicesFound", { count: devices.length }) : t("settings.audio.waitingScan")}</span></div>
            <button className="secondary-button" type="button" onClick={() => void onRefresh()}><RefreshCw size={15} />{t("settings.audio.rescan")}</button>
          </div>

          {deviceErrors.map((message) => (
            <p className="settings-validation-error" role="alert" key={message}>
              <TriangleAlert size={15} />{message}
            </p>
          ))}
          <DeviceGroup
            icon={<Volume2 size={18} />}
            title={t("settings.audio.otherVoice")}
            note={t("settings.audio.otherVoiceDescription")}
            devices={outputDevices}
            devicesReady={devicesReady}
            selectedDeviceId={draft.audio.output.mode === "system" ? draft.audio.output.device_id : null}
            specialRows={[
              {
                key: "system",
                name: t("settings.audio.systemOutput"),
                description: t("settings.audio.systemOutputDescription"),
                chosen: draft.audio.output.mode === "system" && draft.audio.output.device_id === null,
                onSelect: () => applySettings((current) => ({
                  ...current,
                  audio: { ...current.audio, output: { mode: "system", device_id: null } },
                })),
              },
              {
                key: "vrchat",
                name: "VRChat",
                description: t("settings.audio.vrchatDescription"),
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
            title={t("settings.audio.ownVoice")}
            note={t("settings.audio.ownVoiceDescription")}
            devices={microphoneDevices}
            devicesReady={devicesReady}
            selectedDeviceId={draft.audio.microphone.mode === "device" ? draft.audio.microphone.device_id : null}
            specialRows={[
              {
                key: "default",
                name: t("settings.audio.defaultMicrophone"),
                description: t("settings.audio.defaultMicrophoneDescription"),
                chosen: draft.audio.microphone.mode === "default",
                onSelect: () => applySettings((current) => ({
                  ...current,
                  audio: { ...current.audio, microphone: { mode: "default", device_id: null } },
                })),
              },
              {
                key: "disabled",
                name: t("settings.audio.disableMicrophone"),
                description: t("settings.audio.disableMicrophoneDescription"),
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
            <div><BookOpen size={18} /><h2>{t("settings.dictionary.title")}</h2><span>{dictionaries.length ? t("settings.dictionary.importedCount", { count: dictionaries.length }) : t("settings.dictionary.noneImported")}</span></div>
            <button className="secondary-button" type="button" disabled={dictionaryBusy} onClick={() => dictionaryFileRef.current?.click()}><Upload size={15} />{t("settings.dictionary.import")}</button>
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
                    <span>{dictionary.source_language.toUpperCase()}{dictionary.target_language ? ` → ${dictionary.target_language.toUpperCase()}` : ""} · {t("settings.dictionary.entryCount", { count: dictionary.entry_count, formatted: new Intl.NumberFormat(locale).format(dictionary.entry_count) })} · {dictionary.revision}</span>
                  </div>
                  <button type="button" disabled={dictionaryBusy} aria-label={t("settings.dictionary.removeNamed", { title: dictionary.title })} title={t("settings.dictionary.remove")} onClick={() => void removeDictionary(dictionary)}><Trash2 size={16} /></button>
                </div>
              ))}
            </div>
          ) : <p className="dictionary-empty">{t("settings.dictionary.emptyHint")}</p>}
          {dictionaryMessage && <p className="dictionary-feedback" role="status">{dictionaryMessage}</p>}
        </div>
      )}

      {activeCategory === "anki" && (
        <div className="settings-section settings-section-active anki-section" id="settings-panel-anki" role="tabpanel" aria-labelledby="settings-tab-anki">
          <div className="section-heading">
            <div>
              <PlusCircle size={18} />
              <h2>{t("settings.anki.title")}</h2>
              <span>{t("settings.anki.subtitle")}</span>
            </div>
            <button className="secondary-button" type="button" disabled={ankiBusy} onClick={() => void loadAnkiStatus()}>
              <RefreshCw className={ankiBusy ? "spin" : ""} size={15} />
              {ankiBusy ? t("common.checking") : t("common.checkAgain")}
            </button>
          </div>

          <div className={`anki-connection ${ankiBusy ? "checking" : ankiStatus?.connected ? (ankiStatus.configuration_valid ? "ready" : "needs-setup") : "offline"}`} aria-live="polite">
            <span className="anki-connection-dot" aria-hidden="true" />
            <div>
              <strong>
                {ankiBusy
                  ? t("settings.anki.checking")
                  : ankiStatus?.connected
                    ? ankiStatus.configuration_valid
                      ? t("settings.anki.ready")
                      : t("settings.anki.needsSetup")
                    : t("settings.anki.offline")}
              </strong>
              <p>{ankiMessage || t("settings.anki.startHint")}</p>
            </div>
            <code>
              {ankiStatus?.version ? `API v${ankiStatus.version}` : `127.0.0.1:${draft.anki.port}`}
            </code>
          </div>

          <div className="anki-endpoint-row">
            <div>
              <span>{t("settings.anki.address")}</span>
              <strong>127.0.0.1</strong>
              <small>{t("settings.anki.addressDescription")}</small>
            </div>
            <label className="anki-port-field">
              <span>{t("settings.anki.port")}</span>
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
            {ankiPortError || t("settings.anki.portHint")}
          </p>

          <div className="form-grid anki-mapping-grid">
            <DeckTreeSelect
              label={t("settings.anki.deck")}
              helper={ankiStatus?.connected ? t("settings.anki.deckDescription") : t("settings.anki.deckOffline")}
              value={draft.anki.deck}
              decks={ankiDeckNames}
              disabled={!ankiStatus?.connected || ankiBusy || saveState === "saving"}
              onChange={(value) => updateAnki("deck", value)}
            />
            <Select
              label={t("settings.anki.noteType")}
              helper={ankiStatus?.connected ? t("settings.anki.noteTypeDescription") : t("settings.anki.noteTypeOffline")}
              value={draft.anki.model}
              options={ankiModelOptions}
              disabled={!ankiStatus?.connected || ankiBusy || saveState === "saving"}
              onChange={(value) => updateAnki("model", value)}
            />
            <Select
              label={t("settings.anki.frontField")}
              helper={t("settings.anki.frontFieldDescription")}
              value={draft.anki.front_field}
              options={ankiFieldOptions}
              disabled={!ankiStatus?.connected || !ankiStatus.fields.length || ankiBusy || saveState === "saving"}
              onChange={(value) => updateAnki("front_field", value)}
            />
            <Select
              label={t("settings.anki.backField")}
              helper={t("settings.anki.backFieldDescription")}
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
            <div><Wrench size={18} /><h2>Debug</h2><span>{t("settings.debug.subtitle")}</span></div>
            <p>{t("settings.debug.readOnly")}</p>
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

function RangeField({ label, helper, value, min, max, step, disabled, formatValue, onCommit }: {
  label: string;
  helper: string;
  value: number;
  min: number;
  max: number;
  step: number;
  disabled: boolean;
  formatValue: (value: number) => string;
  onCommit: (value: number) => void;
}) {
  const { t } = useTranslation();
  const [draftValue, setDraftValue] = useState(value);
  const draftValueRef = useRef(value);
  const committedValueRef = useRef(value);
  const progress = ((draftValue - min) / (max - min)) * 100;

  useEffect(() => {
    draftValueRef.current = value;
    committedValueRef.current = value;
    setDraftValue(value);
  }, [value]);

  const commit = () => {
    const next = draftValueRef.current;
    if (next === committedValueRef.current) return;
    committedValueRef.current = next;
    onCommit(next);
  };

  return (
    <label className={`range-field ${disabled ? "disabled" : ""}`}>
      <span className="range-field-header">
        <span>{label}</span>
        <output aria-label={t("common.currentValue", { label })}>{formatValue(draftValue)}</output>
      </span>
      <input
        className="range-input"
        type="range"
        min={min}
        max={max}
        step={step}
        value={draftValue}
        disabled={disabled}
        aria-label={label}
        aria-valuetext={formatValue(draftValue)}
        style={{ "--range-progress": `${progress}%` } as CSSProperties}
        onChange={(event) => {
          const next = Number(event.target.value);
          draftValueRef.current = next;
          setDraftValue(next);
        }}
        onPointerUp={commit}
        onPointerCancel={commit}
        onKeyUp={commit}
        onBlur={commit}
      />
      <span className="range-bounds" aria-hidden="true">
        <span>{formatValue(min)}</span>
        <span>{formatValue(max)}</span>
      </span>
      <small>{helper}</small>
    </label>
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
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const [open, setOpen] = useState(false);
  const [expandedNames, setExpandedNames] = useState<Set<string>>(
    () => new Set(ankiDeckAncestors(value)),
  );
  const [activeName, setActiveName] = useState(value);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const itemRefs = useRef(new Map<string, HTMLButtonElement>());
  const tree = useMemo(() => buildAnkiDeckTree(decks, locale), [decks, locale]);
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

  useDismissibleLayer(open, rootRef, closeAndFocusTrigger);

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
                        aria-label={t(
                          node.expanded ? "common.collapseNamed" : "common.expandNamed",
                          { name: node.label },
                        )}
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

  useDismissibleLayer(open, rootRef, () => setOpen(false));

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
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
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
            description={t("settings.audio.deviceDescription", {
              sampleRate: new Intl.NumberFormat(locale).format(device.sample_rate),
              channels: device.channels,
              defaultSuffix: device.is_default ? t("settings.audio.defaultSuffix") : "",
            })}
            chosen={selectedDeviceId === device.id}
            disabled={disabled}
            onSelect={() => onSelectDevice(device.id)}
          />
        ))}
        {!devicesReady
          ? <p className="device-empty">{t("settings.audio.scanning")}</p>
          : !devices.length && <p className="device-empty">{t("settings.audio.noDevices")}</p>}
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
  const { t } = useTranslation();
  return (
    <nav className="bottom-dock" aria-label={t("navigation.main")}>
      <DockButton label={t("navigation.live")} active={page === "live"} onClick={() => onPageChange("live")}><MessageSquare /></DockButton>
      <DockButton label={t("navigation.history")} active={page === "history"} onClick={() => onPageChange("history")}><History /></DockButton>
      <DockButton label={t("navigation.settings")} active={page === "settings"} onClick={() => onPageChange("settings")}><SlidersHorizontal /></DockButton>
      <i className="dock-divider" aria-hidden="true" />
      <DockButton label={t("navigation.compact")} tonal onClick={onCompact}><Shrink /></DockButton>
      <DockButton label={t(running ? "capture.stop" : "capture.start")} primary onClick={onCapture}>{running ? <Square /> : <Mic />}</DockButton>
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

function DictionaryPopover({ lookup, compact = false, onClose }: { lookup: Lookup; compact?: boolean; onClose: () => void }) {
  const { t } = useTranslation();
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
    const cardContent = ankiDictionaryContent(visibleEntries, t("dictionary.builtIn"));
    setAnkiState("adding");
    setAnkiFeedback("");
    try {
      const result = await coreApi.createCard({
        term: lookup.term,
        reading: entry.reading,
        definition: cardContent.definition,
        context: lookup.context,
        dictionary: cardContent.dictionary,
        language: entry.language,
        labels: {
          definition: t("anki.card.definition"),
          context: t("anki.card.context"),
        },
      });
      setAnkiState("success");
      setAnkiFeedback(t("dictionary.anki.created", {
        noteId: result.note_id,
        count: visibleEntries.length,
      }));
    } catch (reason) {
      setAnkiState("error");
      setAnkiFeedback(localizedError(reason, t, "errors.anki.createCard"));
    }
  };

  return (
    <div ref={ref} className={`dictionary-popover ${compact ? "compact-inline-dictionary" : `popover-${placement.side}`}`} style={style as CSSProperties} role="dialog" aria-label={t("dictionary.dialogLabel", { term: lookup.term })}>
      <div className="dictionary-header">
        <div><h2>{lookup.term}</h2>{entry?.reading && <span className="reading">{entry.reading}</span>}{entry && <span className="language-chip">{entry.language.toUpperCase()}</span>}</div>
        <button type="button" aria-label={t("dictionary.close")} onClick={onClose}><X size={19} /></button>
      </div>
      <div className="dictionary-scroll">
        {visibleEntries.length ? (
          <div className="dictionary-definitions">
            {visibleEntries.map((item, index) => (
              <article className="dictionary-definition-item" key={`${item.dictionary ?? "local"}-${item.term}-${item.reading ?? ""}-${index}`}>
                <div className="dictionary-entry-meta">
                  <span className="dictionary-source-name">{item.dictionary || t("dictionary.builtIn")}</span>
                  {visibleEntries.length > 1 && <span className="dictionary-entry-index">{String(index + 1).padStart(2, "0")}</span>}
                </div>
                <ol className="definition-glosses">
                  {definitionGlosses(item.definition).map((gloss, glossIndex) => <li key={`${gloss}-${glossIndex}`}>{gloss}</li>)}
                </ol>
              </article>
            ))}
          </div>
        ) : <p className="definition muted">{t("dictionary.noDefinitions")}</p>}
        <div className="lookup-context"><span>{t("dictionary.context")}</span><q>{contextExcerpt(lookup.context, lookup.term)}</q></div>
      </div>
      <button className={`anki-button anki-state-${ankiState}`} type="button" disabled={!entry || ankiState === "adding" || ankiState === "success"} onClick={() => void add()}>
        {ankiState === "success"
          ? <Check size={16} />
          : ankiState === "error"
            ? <TriangleAlert size={16} />
            : <PlusCircle size={16} />}
        {ankiButtonLabel(ankiState, (key) => t(key))}
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
  const { t } = useTranslation();
  const captureLabel = t(running ? "capture.pause" : "capture.start");
  return (
    <div className="compact-shell">
      <div className="compact-drag-region" data-tauri-drag-region />
      <div className="compact-status"><i className={running ? "running" : ""} />{subtitle?.language?.toUpperCase() ?? "AUTO"}</div>
      <p onMouseUp={() => subtitle && void onSelect(subtitle.text)}>{subtitle?.text ?? t("live.waiting")}</p>
      <div className="compact-actions">
        <button className={`compact-capture-button ${running ? "running" : ""}`} type="button" aria-label={captureLabel} title={captureLabel} onClick={onCapture}>
          {running ? <Square size={15} /> : <Mic size={16} />}
        </button>
        <button className="compact-secondary-action" type="button" aria-label={t("window.restore")} title={t("window.restore")} onClick={onRestore}><MessageSquare size={17} /></button>
        <button className="compact-secondary-action" type="button" aria-label={t("window.close")} title={t("window.close")} onClick={onClose}><X size={17} /></button>
      </div>
    </div>
  );
}

export default App;
