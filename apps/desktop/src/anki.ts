export type AnkiAddState = "idle" | "adding" | "success" | "error";

type TranslateAnki = (key: string) => string;

const defaultLabel: TranslateAnki = (key) => ({
  "dictionary.anki.adding": "正在添加…",
  "dictionary.anki.success": "已添加到 Anki",
  "dictionary.anki.retry": "重试添加",
  "dictionary.anki.add": "添加到 Anki",
})[key] ?? key;

export function ankiButtonLabel(state: AnkiAddState, translate: TranslateAnki = defaultLabel): string {
  if (state === "adding") return translate("dictionary.anki.adding");
  if (state === "success") return translate("dictionary.anki.success");
  if (state === "error") return translate("dictionary.anki.retry");
  return translate("dictionary.anki.add");
}
