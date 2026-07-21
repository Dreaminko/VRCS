# Page dependency trees

All four views share a single React entry and render branch.

## 实时字幕 (`live`)

Entry: `apps/desktop/src/App.tsx`

- `apps/desktop/src/App.tsx`
  - `apps/desktop/src/api.ts`
    - `apps/desktop/src/types.ts`
  - `apps/desktop/src/types.ts`
- `apps/desktop/src/main.tsx`
  - `apps/desktop/src/App.tsx`
  - `apps/desktop/src/styles.css`
- `apps/desktop/src-tauri/app-icon.svg`

## 字幕历史 (`history`)

Entry: `apps/desktop/src/App.tsx`

- Same complete dependency tree as `live`; uses the `history` conditional branch and `SubtitleList`.

## 识别设置 (`asr`)

Entry: `apps/desktop/src/App.tsx`

- Same complete dependency tree as `live`; uses `SettingsPanel` and `Select`.

## 音频设备 (`audio`)

Entry: `apps/desktop/src/App.tsx`

- Same complete dependency tree as `live`; uses `AudioPanel`.

