export type AnkiAddState = "idle" | "adding" | "success" | "error";

export function ankiButtonLabel(state: AnkiAddState): string {
  if (state === "adding") return "正在添加…";
  if (state === "success") return "已添加到 Anki";
  if (state === "error") return "重试添加";
  return "添加到 Anki";
}
