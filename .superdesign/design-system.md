# VRCS clean redesign system

## Product context

VRCS is a Windows desktop utility for local VRChat transcription and language learning. The main task is to start capture, glance at live subtitles, confirm capture health, select a word, read its meaning, and create an Anki card with minimal interruption.

The product has three primary views: live subtitles, subtitle history, and one unified settings page that contains both ASR recognition configuration and speaker/microphone device selection. A contextual dictionary panel appears after text selection. The app can also switch into a compact subtitle-only window while capture is running. The default desktop viewport is 1180 × 760 and the minimum supported full-window viewport is 860 × 620.

## Redesign intent

Start from zero. Do not preserve the existing dark technical dashboard, neon-green treatment, sharp HUD-like panels, radial background, or current sidebar styling. Keep only the app's functions, Chinese copy, and the recognizable V brand mark.

The new UI must feel clean, quiet, lightweight, and immediately understandable. It should resemble a focused native productivity tool rather than a gaming overlay or developer console. Use a cohesive light-blue Material You / Material 3 inspired theme across every view. Subtitles are the primary content. Status and settings should remain available without competing for attention.

## Color tokens

- App canvas: `#F6F9FC`
- Main surface: `#FFFFFF`
- Subtle surface: `#EDF4FA`
- Elevated surface: `#FAFCFE`
- Primary text: `#18232D`
- Secondary text: `#5F6F7D`
- Quiet text: `#8796A3`
- Border: `#D8E3EC`
- Border strong: `#C2D1DE`
- Primary accent: `#3D73A8`
- Primary accent hover: `#315F8B`
- Primary accent soft: `#DEEDFA`
- Primary accent softer: `#EDF6FD`
- Info: `#416F99`
- Info soft: `#E5F0F8`
- Warning: `#9A6A25`
- Error: `#A8463D`
- Error soft: `#F8EAE8`

The blue family is the only brand accent. Use pale blue mainly for surfaces, selected containers, focus rings, and hover states; reserve the stronger blue for primary actions, icons, and short emphasis. Do not use gradients, neon colors, pure black surfaces, glassmorphism, decorative glow, purple, teal, or green brand accents.

## Typography

- Body and interface: `"Segoe UI", "Microsoft YaHei UI", sans-serif`
- Technical metadata only: `"Cascadia Mono", "SFMono-Regular", monospace`
- Page title: 26 px, weight 650, line-height 1.25
- Section title: 15 px, weight 650
- Subtitle text: 16 px, weight 400, line-height 1.7
- Body/control text: 13 px, line-height 1.5
- Metadata: 11 px, line-height 1.4

Avoid all-uppercase headings except the four-letter product name. Do not use wide letter spacing for decorative labels.

## Spacing and geometry

Use a 4 px base rhythm. Preferred spacing values: 4, 8, 12, 16, 20, 24, and 32 px. Use 16 to 20 px radii for prominent Material You surfaces, 10 to 12 px for compact panels, 8 px for controls, and full pills for status tags and the floating bottom navigation container. Keep one-pixel borders. Use a single soft overlay shadow: `0 16px 48px rgba(40, 76, 110, 0.12)`.

## Layout principles

- Do not use a persistent left or right sidebar or a top navigation bar. Place the three primary destinations in one centered floating pill navigation dock near the bottom edge of the window.
- Adapt Material You / Material 3 navigation language for desktop: a quiet elevated surface with three equal icon-only destinations (`实时字幕`, `字幕历史`, `设置`) and a soft accent capsule for the selected destination. Remove all persistent explanatory text from the bottom dock, including destination labels and action labels. Provide each icon with a Chinese accessible name and a concise hover/focus tooltip. Do not imitate an Android system bar literally.
- Synchronize the bottom dock across every full-window view. Live subtitles, subtitle history, and unified settings must use the same dock height, width, destination order, icon set, spacing, capture/compact action placement, and interaction states; only the selected destination and capture state may change.
- Keep the dock visually detached from the window edge with 16 to 24 px clearance. Reserve at least 104 px of content padding below scrollable views so subtitles and controls are never covered.
- Keep lightweight page identity and the V brand mark in the content header, separate from navigation.
- Do not use a dashboard-card grid or a permanently visible context rail.
- Give the transcript most of the window height and width.
- Put the capture action in one consistent, obvious location.
- Group live system status into one quiet summary area instead of four equally prominent dashboard cards.
- Use one large unified settings surface with internal sections and one clear save action. Do not split recognition and audio into separate pages, tabs, or separate dashboard cards.
- Open dictionary results as a temporary selection-anchored contextual popover after text selection. Position it beside the selected word without covering that word, keep it above every app element including the bottom dock, and never turn it into a drawer, permanent sidebar, or modal overlay.
- Preserve the live and history views; merge the previous `识别设置` and `音频设备` destinations into one `设置` destination.
- At 860 px width, all core controls and subtitle text remain usable without horizontal scrolling.
- Provide a secondary compact-mode action immediately beside the capture action in the bottom dock. It must read as a window/view control, not a fifth navigation destination or a second primary action.

## View patterns

- Live subtitles: lightweight identity header, one inline capture-health summary, then a dominant chronological IM-style conversation stream. Render speaker/output capture as left-aligned `对方` bubbles and microphone/input capture as right-aligned `我` bubbles. Make text selection and the dictionary action discoverable without persistent instructional clutter.
- Subtitle history: retain the same transcript rhythm, add a compact result count and lightweight language/date filtering without turning the page into a data dashboard.
- Unified settings: use one large centered white/elevated surface, up to the 980px workspace width, with two internally divided sections rather than separate cards or tabs. The first section is `识别引擎` with four clearly labeled selects, a quiet model-status note, and explicit disabled-during-capture state. The second section is `音频来源` with distinct `系统音频输出 · 对方` and `麦克风输入 · 我` device groups, roomy radio-selectable rows, scan status, and a secondary `重新扫描` action. Finish with one shared action row and one `保存设置` button for the whole page.
- Dictionary: open as a temporary elevated popover anchored 8 to 12px from the selected word. Prefer placement above or upper-right of the selection; flip below or clamp horizontally when the viewport edge leaves insufficient room. Keep the selected word visible and highlighted, use no backdrop, and render the popover at the highest app layer above the bottom dock, window controls, and conversation content. Order content as term, reading/language, concise definition, source context, and Anki action. It must remain closable and never become permanent navigation.
- Compact subtitle mode: switch to a small solid-surface always-on-top window, approximately 720 × 120 px, showing only the current streaming subtitle as dominant selectable text. Omit navigation, page title, metrics, history, dictionary, settings, and decorative chrome. Keep a tiny capture-state dot and language label; reveal restore-to-full-window and close controls on hover/focus. The surface itself should provide a generous drag region. Never use transparency or glass blur behind subtitle text.
- Errors, empty states, loading, connected, disconnected, capturing, and stopped states must use the same blue-neutral component language and reserve red only for actual errors or destructive stop feedback.

## Component rules

- Primary button: accent fill, white text, 40 px minimum height, 8 px radius or full pill when embedded in the bottom dock.
- Secondary button: white surface, strong border, primary text.
- Bottom navigation dock: 56 to 64 px high, white or elevated surface, one-pixel border, full-pill radius, soft overlay shadow, and a maximum width that remains comfortable at the 860 px minimum viewport.
- Navigation item: 40 to 44 px square, one simple 20 to 22 px icon, no visible text, and a soft-accent circular or short-pill selected surface. Use recognizable icons consistently: message bubbles for `实时字幕`, history/clock for `字幕历史`, and sliders/settings for `设置`. Selection must not change the dock's outer dimensions. Each item requires a tooltip and accessible name.
- Capture action: integrate one clear accent circular or compact pill icon action at the right end of the dock, separated from destination switching without introducing a second floating control cluster. Show only a microphone/start icon while stopped and a square/stop icon while capturing; expose `开始转写` or `停止转写` through tooltip and accessible name, not persistent text.
- Compact-mode button: 40 to 48 px square, secondary tonal-blue surface, recognizable shrink/window icon, no visible text, visible focus state, and tooltip/accessible name `字幕模式`. Place it directly before the capture action.
- Bottom dock tooltips: appear above the hovered or keyboard-focused icon after a short delay, use a compact dark-neutral or strong-blue surface with high-contrast text, never alter layout, and never cover the associated icon.
- Status: dot plus readable text. Never rely on color alone.
- Live conversation bubble: maximum width about 72% of the conversation surface. `对方` uses a left-aligned white/elevated bubble with a speaker icon; `我` uses a right-aligned primary-accent-soft bubble with a microphone icon. Both use 16px selectable subtitle text, 16 to 20px bubble radius, comfortable padding, and compact time/language/source metadata outside or above the text. Differentiate sources with alignment, icon, and readable labels, never color alone. Consecutive messages from the same source may group more tightly.
- Streaming bubble: the current partial sentence stays at the bottom of the conversation, uses the correct source alignment, and shows a quiet `转写中` label or trailing ellipsis without continuous decorative animation.
- History transcript row: quiet timestamp, dominant sentence, compact language tag, generous vertical breathing room. History may remain a dense neutral list rather than chat bubbles.
- Panel: white or elevated surface with border; no stacked decorative containers.
- Form control: 40 px height, visible label above, white background, clear focus ring.
- Unified settings surface: one 20px-radius main container with quiet section headers and one-pixel internal dividers. Use whitespace and dividers for grouping, not nested cards. At wide desktop sizes the recognition controls may use a two-column grid; device rows remain full-width. At 860px width, all sections stack without horizontal scrolling.
- Dictionary popover: 320 to 360px wide, 16 to 20px radius, white/elevated surface, strong border, highest app z-index, restrained overlay shadow, and a small tonal arrow pointing toward the selection. Keep the header compact with term, reading, language pill, and close control. Follow with a concise definition, short source context, and one full-width Anki action. Dismiss on Escape, outside click, or close; keep focus visible and never trap the user behind a backdrop.
- Empty state: one short sentence and, if useful, one direct action. No illustration required.

## Motion and accessibility

Use 120 to 180 ms ease-out transitions for navigation, hover, focus, and contextual panel entry. New subtitles may fade in once without continuous motion. Respect `prefers-reduced-motion`.

Maintain WCAG AA contrast, visible keyboard focus, 36 px minimum interactive targets, semantic labels, and selected states expressed with both color and shape/text. Subtitle text must remain selectable.
