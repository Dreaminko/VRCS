# Localization contributions

VRCS currently ships with Simplified Chinese (`zh-CN`), Japanese (`ja-JP`), and English (`en-US`). Locale files are discovered automatically, so a new translation normally changes only one JSON file.

## Add a language

From the repository root, install dependencies and generate a locale file:

```powershell
npm install
npm run i18n:new -- fr-FR "Français"
```

Use a canonical [BCP 47](https://www.rfc-editor.org/rfc/bcp/bcp47.txt) locale and the language’s native display name. For a right-to-left language, pass `rtl` as the third argument:

```powershell
npm run i18n:new -- ar "العربية" rtl
```

The command creates `apps/desktop/src/i18n/locales/<locale>.json` with the current English key structure and empty values. Translate every value before submitting. No TypeScript registration is required.

## Locale file contract

Each file contains metadata and a nested `translation` object:

```json
{
  "_meta": {
    "locale": "fr-FR",
    "name": "Français",
    "direction": "ltr",
    "status": "complete"
  },
  "translation": {
    "common": {
      "close": "Fermer"
    }
  }
}
```

Requirements:

- The filename must exactly match `_meta.locale`.
- `_meta.locale` must use canonical BCP 47 casing.
- `_meta.name` must be the language name written in that language.
- `_meta.direction` must be `ltr` or `rtl`.
- `_meta.status` must be `complete` before the file is submitted.
- Keep every key from `en-US.json`; do not add or rename keys in a translation-only change.
- Translate values only. Keep product names such as VRCS, VRChat, Whisper, CUDA, Anki, and AnkiConnect unchanged unless the target community has an established spelling.
- Preserve interpolation variables exactly, including both braces: `{{count}}`, `{{name}}`, `{{port}}`, and so on.
- Keep JSON UTF-8 encoded and do not add comments or trailing commas.
- Translate for the interface context rather than word for word. Prefer short labels and direct error messages.

English (`en-US.json`) is the source-of-truth key set and runtime fallback. Diagnostics in the Core `detail` field are for logs; user-facing errors are translated through stable error codes.

## Validate the contribution

Run:

```powershell
npm run check:i18n
npm --workspace apps/desktop test
npm run build:frontend
```

`check:i18n` verifies:

- valid JSON and locale metadata;
- matching filename and locale;
- exact key parity with English;
- non-empty translations;
- matching interpolation variables;
- translation coverage for statically referenced UI keys and required runtime error/status keys.

The Localization GitHub Actions workflow runs the same validator, desktop tests, and frontend build for localization pull requests.

## Review in the application

Start the desktop app, then select the language under **Settings → System → Interface language**. Review at least:

- live subtitles, conversation sidebar, history filters, and compact mode;
- every settings category;
- warnings, validation messages, and Anki connection states;
- long labels at narrow window widths;
- date and number formatting;
- the system tray’s Show/Quit labels;
- Anki card Definition/Context headings.

For RTL languages, also verify reading order, controls, icons, popovers, and mixed Latin/product-name content. The document direction is applied automatically, but layout-specific corrections may still be needed in the same contribution.

## Updating an existing translation

Edit its locale file directly and run the same three checks. When English gains a key, the validator reports every locale that needs the corresponding translation.

---

## 中文摘要

新增语言只需运行 `npm run i18n:new -- <语言代码> "<语言本名>"`，翻译生成的单个 JSON 文件，不需要修改 TypeScript 注册表。提交前必须运行 `npm run check:i18n`、桌面端测试和前端构建；请完整保留所有键与 `{{插值变量}}`，并在应用内检查长文案、托盘菜单和 Anki 卡片标签。
