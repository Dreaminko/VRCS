const graphemes = new Intl.Segmenter(undefined, { granularity: "grapheme" });

// Keep the newest complete graphemes at the configured font size.
export function visibleTail(text: string, fits: (candidate: string) => boolean): string {
  if (!text || fits(text)) return text;
  if (!fits("…")) return "";
  const starts = [...graphemes.segment(text)].map(({ index }) => index).concat(text.length);
  let low = 0;
  let high = starts.length - 1;
  while (low < high) {
    const middle = Math.floor((low + high) / 2);
    if (fits(`…${text.slice(starts[middle])}`)) high = middle;
    else low = middle + 1;
  }
  let start = starts[low];
  const fitted = `…${text.slice(start)}`;
  if (start > 0 && /[a-z0-9]/i.test(text[start - 1]) && /[a-z0-9]/i.test(text[start] || "")) {
    const boundary = /\s+\S/.exec(text.slice(start));
    if (boundary) start += boundary.index + boundary[0].length - 1;
  }
  const readable = `…${text.slice(start).trimStart()}`;
  return fits(readable) ? readable : fitted;
}

export function visibleCaption(text: string, fits: (candidate: string) => boolean): string {
  const label = /^\[\d+\] /.exec(text)?.[0];
  if (!label) return visibleTail(text, fits);
  const caption = label + visibleTail(text.slice(label.length), (candidate) => fits(label + candidate));
  return fits(caption) ? caption : fits(label.trimEnd()) ? label.trimEnd() : "";
}
