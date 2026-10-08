export interface DesktopOcrBlock {
  source: { id: number; text: string; confidence: number; polygon: number[][] };
  translations: Array<{ target_language: string; text: string | null; error_code: string | null }>;
}

export interface DesktopOcrStatus {
  scan_id: number;
  revision: number;
  state: "disabled" | "idle" | "capturing" | "selecting" | "recognizing" | "translating" | "complete" | "no_text" | "partial_failure" | "error";
  blocks: DesktopOcrBlock[];
  error: string | null;
  shortcut_error: string | null;
  timed_out: boolean;
}

export function applyDesktopOcrStatus(current: DesktopOcrStatus | null, next: DesktopOcrStatus): DesktopOcrStatus {
  if (current && (next.scan_id < current.scan_id ||
    (next.scan_id === current.scan_id && next.revision <= current.revision))) return current;
  return next;
}

export function desktopOcrCopyText(blocks: DesktopOcrBlock[]): string {
  return blocks.map((block) => [block.source.text, ...block.translations.map((translation) => translation.text)]
    .filter((text): text is string => Boolean(text)).join("\n")).join("\n\n");
}
