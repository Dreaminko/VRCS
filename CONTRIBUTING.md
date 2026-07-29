# Contributing to VRCS

Thank you for helping improve VRCS.

## Localization

Adding a language is intentionally a file-only contribution: create one locale JSON file, translate it, and submit it with the localization checks passing. The application discovers valid locale files automatically.

See the [localization contribution guide](LOCALIZATION.md) for the generator command, translation rules, validation, and review checklist.

## Development checks

Install dependencies once:

```powershell
npm install
```

Run the checks relevant to your change:

```powershell
npm run check:i18n
npm --workspace apps/desktop test
npm run build:frontend
.\scripts\test-core.ps1
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

Please keep commits focused and do not include generated build output.
