# Extractable components

## AppSidebar

- Source: `apps/desktop/src/App.tsx`
- Category: layout
- Description: Fixed left navigation with VRCS brand and Core connection indicator.
- Extractable props: `activeItem` (string, default `live`), `isConnected` (boolean, default `true`)
- Hardcoded: brand mark, labels, navigation order, colors, typography

## AppHeader

- Source: `apps/desktop/src/App.tsx`
- Category: layout
- Description: Page heading and the primary start/stop transcription action.
- Extractable props: `currentPage` (string, default `实时字幕`), `isRunning` (boolean, default `false`)
- Hardcoded: eyebrow, button labels, spacing and type scale

## SubtitleList

- Source: `apps/desktop/src/App.tsx`
- Category: basic
- Description: Timestamped transcript rows reused by live and history views.
- Extractable props: none; content is data-driven and should remain inline in drafts
- Hardcoded: row structure, language placement, timestamp column

## LookupPanel

- Source: `apps/desktop/src/App.tsx`
- Category: basic
- Description: Dictionary and Anki side panel.
- Extractable props: `isOpen` (boolean, default `true`)
- Hardcoded: definition layout, context blockquote, Anki action

