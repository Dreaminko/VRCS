# Routes and views

The app is a single-window React SPA without a routing library. `App.tsx` switches views through local `page` state.

| View key | Label | Rendered component/branch | Shared layout |
| --- | --- | --- | --- |
| `live` | 实时字幕 | Live metrics and `SubtitleList` | `App` shell |
| `history` | 字幕历史 | Full `SubtitleList` | `App` shell |
| `asr` | 识别设置 | `SettingsPanel` | `App` shell |
| `audio` | 音频设备 | `AudioPanel` | `App` shell |

Entry: `apps/desktop/src/main.tsx`

```tsx
createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
```

View configuration from `apps/desktop/src/App.tsx`:

```tsx
type Page = "live" | "history" | "asr" | "audio";

const pageNames: Record<Page, string> = {
  live: "实时字幕",
  history: "字幕历史",
  asr: "识别设置",
  audio: "音频设备",
};
```

