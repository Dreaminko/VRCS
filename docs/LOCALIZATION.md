# Localization contributions

VRCS currently ships with English (`en-US`), Simplified Chinese (`zh-CN`), Traditional Chinese (`zh-Hant`), and Japanese (`ja-JP`). Locale files are discovered automatically, so adding a new translation only requires creating a locale file and registering its metadata.

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

The command creates `apps/desktop/src/i18n/locales/<locale>.json` with the current English key structure and empty values, and registers its metadata in `apps/desktop/src/i18n/locales-meta.json`. Translate every value before submitting; no TypeScript code changes are required.

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
- Translate values only. Keep product and technical names such as VRCS, VRChat, Qwen3 ASR, PLaMo, UniText, Anki, and AnkiConnect unchanged unless the target community has an established spelling.
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
- `apps/desktop/src/i18n/locales-meta.json` kept in sync with locale `_meta` entries;
- exact key parity with English;
- non-empty translations;
- matching interpolation variables;
- translation coverage for statically referenced UI keys and required runtime error/status keys.

CI runs the same validator, desktop tests, and frontend build for localization pull requests.

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
