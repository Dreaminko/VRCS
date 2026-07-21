# Theme and design tokens

- Styling: global vanilla CSS
- Component library: none, custom React primitives
- Theme mode: dark only
- Font stack: Segoe UI / Microsoft YaHei UI; Cascadia Mono for technical labels
- Brand asset: `apps/desktop/src-tauri/app-icon.svg`

Full source of `apps/desktop/src/styles.css`:

```css
:root {
  font-family: "Segoe UI", "Microsoft YaHei UI", sans-serif;
  color: #e8ece9;
  background: #0d100f;
  font-synthesis: none;
  --green: #a8f56b;
  --panel: #171b19;
  --line: #2a302d;
  --muted: #8f9993;
}

* { box-sizing: border-box; }
body { margin: 0; min-width: 860px; min-height: 100vh; background: radial-gradient(circle at 70% -20%, #26342b 0, transparent 38%), #0d100f; }
button, select { font: inherit; }
button { color: inherit; }

.shell { min-height: 100vh; display: grid; grid-template-columns: 210px 1fr; }
aside { position: fixed; inset: 0 auto 0 0; width: 210px; display: flex; flex-direction: column; padding: 26px 18px; border-right: 1px solid var(--line); background: rgba(13, 16, 15, .92); }
.brand { display: flex; align-items: center; gap: 11px; font-weight: 700; letter-spacing: .12em; }
.brand > span { display: grid; place-items: center; width: 38px; height: 38px; color: #101410; background: var(--green); font: 500 20px "Cascadia Mono", monospace; clip-path: polygon(10% 0, 100% 0, 90% 100%, 0 100%); }
.brand small { display: block; margin-top: 2px; color: var(--muted); font: 400 8px "Cascadia Mono", monospace; letter-spacing: .08em; }
nav { display: grid; gap: 5px; margin-top: 55px; }
nav button { padding: 11px 14px; border: 0; border-left: 2px solid transparent; background: transparent; color: var(--muted); text-align: left; cursor: pointer; }
nav button:hover { color: #fff; }
nav button.active { color: var(--green); border-color: var(--green); background: linear-gradient(90deg, rgba(168, 245, 107, .11), transparent); }
.connection { margin-top: auto; color: var(--muted); font-size: 12px; }
.connection i { display: inline-block; width: 7px; height: 7px; margin-right: 8px; border-radius: 50%; background: #d05f58; }
.connection.connected i { background: var(--green); box-shadow: 0 0 10px rgba(168, 245, 107, .5); }

main { grid-column: 2; width: min(1050px, calc(100vw - 250px)); margin: 0 auto; padding: 42px 0 70px; }
header { display: flex; align-items: end; justify-content: space-between; margin-bottom: 34px; }
h1 { margin: 3px 0 0; font-size: 32px; letter-spacing: -.04em; }
h2 { margin: 0; font-size: 17px; }
.eyebrow { margin: 0; color: var(--green); font: 500 10px "Cascadia Mono", monospace; letter-spacing: .18em; }
.capture, .primary { border: 1px solid var(--green); background: var(--green); color: #111510; font-weight: 700; cursor: pointer; }
.capture { min-width: 112px; padding: 12px 18px; }
.capture.stop { border-color: #f18b80; background: transparent; color: #f18b80; }
.primary { padding: 11px 20px; }
.primary:disabled { opacity: .35; cursor: not-allowed; }
.full { width: 100%; }
.metrics { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 1px; margin-bottom: 20px; border: 1px solid var(--line); background: var(--line); }
.metric { min-width: 0; padding: 16px 18px; background: #131715; }
.metric span { display: block; margin-bottom: 6px; color: var(--muted); font: 400 10px "Cascadia Mono", monospace; text-transform: uppercase; }
.metric strong { display: block; overflow: hidden; font-size: 13px; text-overflow: ellipsis; white-space: nowrap; }
.panel { border: 1px solid var(--line); background: rgba(23, 27, 25, .88); }
.panel-title { display: flex; align-items: center; justify-content: space-between; padding: 18px 20px; border-bottom: 1px solid var(--line); }
.panel-title span, .muted { color: var(--muted); font-size: 11px; }
.subtitle-list article { display: grid; grid-template-columns: 76px 1fr 30px; gap: 14px; align-items: start; padding: 18px 20px; border-bottom: 1px solid #242a27; }
.subtitle-list article:last-child { border: 0; }
.subtitle-list article:hover { background: rgba(255, 255, 255, .018); }
.subtitle-list time, .subtitle-list em { padding-top: 3px; color: var(--muted); font: 400 10px "Cascadia Mono", monospace; font-style: normal; }
.subtitle-list p { margin: 0; line-height: 1.65; user-select: text; }
.empty { display: grid; min-height: 260px; place-items: center; color: var(--muted); font-size: 13px; }
.empty.small { min-height: 80px; }
.error { display: flex; justify-content: space-between; margin-bottom: 18px; padding: 12px 15px; border: 1px solid #663f3b; background: #2b1b1a; color: #f0a59d; font-size: 13px; }
.error button { border: 0; background: transparent; cursor: pointer; }
.form-panel { padding-bottom: 22px; }
.form-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 20px; padding: 24px 20px; }
.form-grid label > span { display: block; margin-bottom: 8px; color: var(--muted); font-size: 12px; }
select { width: 100%; padding: 10px 12px; border: 1px solid var(--line); border-radius: 0; outline: none; background: #111513; color: #edf2ee; }
select:focus { border-color: var(--green); }
.form-panel > .primary { margin-left: 20px; }
.text-button { border: 0; background: transparent; color: var(--green); cursor: pointer; }
.device-list { display: grid; gap: 8px; padding: 20px; }
.device-list label { display: flex; gap: 12px; padding: 14px; border: 1px solid var(--line); cursor: pointer; }
.device-list label.chosen { border-color: #699b4b; background: rgba(168, 245, 107, .05); }
.device-list input { accent-color: var(--green); }
.device-list strong, .device-list span { display: block; }
.device-list strong { font-size: 13px; }
.device-list span { margin-top: 4px; color: var(--muted); font-size: 11px; }
.lookup-panel { position: fixed; z-index: 5; top: 0; right: 0; width: min(360px, 45vw); height: 100vh; padding: 32px 28px; border-left: 1px solid var(--line); background: #141816; box-shadow: -20px 0 60px rgba(0, 0, 0, .3); }
.lookup-panel h2 { margin: 8px 0 28px; font: 500 30px "Cascadia Mono", monospace; }
.close { position: absolute; top: 20px; right: 20px; border: 0; background: transparent; color: var(--muted); font-size: 24px; cursor: pointer; }
.definition { margin-bottom: 14px; padding: 14px; border-left: 2px solid var(--green); background: #1a201d; }
.definition span { color: var(--green); font: 400 10px "Cascadia Mono", monospace; text-transform: uppercase; }
.definition p { margin: 8px 0 0; font-size: 13px; line-height: 1.5; }
blockquote { margin: 26px 0; padding: 0 0 0 14px; border-left: 1px solid #3b433f; color: var(--muted); font-size: 12px; line-height: 1.6; }
.feedback { display: block; margin-top: 12px; color: var(--muted); text-align: center; }

@media (max-width: 980px) {
  .metrics { grid-template-columns: 1fr 1fr; }
}
```

