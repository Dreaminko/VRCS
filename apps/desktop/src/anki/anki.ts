export type AnkiAddState = "idle" | "adding" | "success" | "error";

type TranslateAnki = (key: string) => string;

const defaultLabel: TranslateAnki = (key) => ({
  "dictionary.anki.adding": "Adding…",
  "dictionary.anki.success": "Added to Anki",
  "dictionary.anki.retry": "Try adding again",
  "dictionary.anki.add": "Add to Anki",
})[key] ?? key;

export function ankiButtonLabel(state: AnkiAddState, translate: TranslateAnki = defaultLabel): string {
  if (state === "adding") return translate("dictionary.anki.adding");
  if (state === "success") return translate("dictionary.anki.success");
  if (state === "error") return translate("dictionary.anki.retry");
  return translate("dictionary.anki.add");
}
