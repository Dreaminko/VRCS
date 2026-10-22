export type Quad = [[number, number], [number, number], [number, number], [number, number]];
export interface OcrSceneBlock { id: number; text: string; quad: Quad; masks: Quad[] }
export interface OcrScene { blocks: OcrSceneBlock[]; excluded: Quad[]; opacity: number; aspect: number }
export interface OcrCard {
  id: number;
  quad: Quad;
  source: Quad;
  width: number;
  height: number;
  fontSize: number;
  lines: string[];
  connected: boolean;
}
type Measure = (text: string, fontSize: number) => number;
type Bounds = [number, number, number, number];
const WIDTH = 2048;
const PADDING = 12;
const LINE_HEIGHT = 1.4;

const bounds = (quad: Quad): Bounds => [Math.min(...quad.map(([x]) => x)), Math.min(...quad.map(([, y]) => y)), Math.max(...quad.map(([x]) => x)), Math.max(...quad.map(([, y]) => y))];
const rectangle = ([l, t, r, b]: Bounds): Quad => [[l, t], [r, t], [r, b], [l, b]];
const edge = (a: number[], b: number[]) => Math.hypot(a[0]! - b[0]!, a[1]! - b[1]!);

function overlaps(a: Quad, b: Quad) {
  for (const quad of [a, b]) {
    for (let index = 0; index < 4; index++) {
      const from = quad[index]!;
      const to = quad[(index + 1) % 4]!;
      const axis = [to[1] - from[1], from[0] - to[0]];
      const project = (point: number[]) => point[0]! * axis[0]! + point[1]! * axis[1]!;
      const ap = a.map(project);
      const bp = b.map(project);
      if (Math.max(...ap) <= Math.min(...bp) + 0.01 || Math.max(...bp) <= Math.min(...ap) + 0.01) return false;
    }
  }
  return true;
}

function wrapText(text: string, width: number, fontSize: number, maxLines: number, measure: Measure): string[] | null {
  const lines: string[] = [];
  for (const paragraph of text.split("\n")) {
    let line = "";
    for (const token of paragraph.match(/\S+|\s+/gu) ?? []) {
      if (measure(line + token, fontSize) <= width) {
        line += token;
        continue;
      }
      if (line.trim()) {
        lines.push(line.trimEnd());
        line = "";
      }
      if (lines.length >= maxLines) return null;
      if (!token.trim()) continue;
      for (const character of token) {
        if (measure(line + character, fontSize) > width) {
          if (!line || lines.length + 1 >= maxLines) return null;
          lines.push(line);
          line = "";
        }
        line += character;
      }
    }
    lines.push(line.trimEnd());
    if (lines.length > maxLines) return null;
  }
  return lines;
}

function fitText(text: string, width: number, height: number, measure: Measure) {
  if (width <= PADDING * 2 || height <= PADDING * 2) return null;
  for (let fontSize = 48; fontSize >= 24; fontSize -= 4) {
    const maxLines = Math.floor((height - PADDING * 2) / (fontSize * LINE_HEIGHT));
    if (maxLines < 1) continue;
    const lines = wrapText(text, width - PADDING * 2, fontSize, maxLines, measure);
    if (lines) return { fontSize, lines, height: lines.length * fontSize * LINE_HEIGHT + PADDING * 2 };
  }
  return null;
}

/** Map an HTML rectangle to the source quadrilateral with a GPU CSS transform. */
export function quadTransform(quad: Quad, width: number, height: number): number[] | null {
  if (!quad.flat().every(Number.isFinite) || width <= 0 || height <= 0) return null;
  const [a, b, c, d] = quad;
  const dx1 = b[0] - c[0], dx2 = d[0] - c[0], dx3 = a[0] - b[0] + c[0] - d[0];
  const dy1 = b[1] - c[1], dy2 = d[1] - c[1], dy3 = a[1] - b[1] + c[1] - d[1];
  const denominator = dx1 * dy2 - dx2 * dy1;
  if (Math.abs(denominator) < 0.00001) return null;
  const g = (dx3 * dy2 - dx2 * dy3) / denominator;
  const h = (dx1 * dy3 - dx3 * dy1) / denominator;
  if ([1, 1 + g, 1 + h, 1 + g + h].some((value) => value <= 0.00001)) return null;
  return [
    (b[0] - a[0] + g * b[0]) / width, (b[1] - a[1] + g * b[1]) / width, 0, g / width,
    (d[0] - a[0] + h * d[0]) / height, (d[1] - a[1] + h * d[1]) / height, 0, h / height,
    0, 0, 1, 0, a[0], a[1], 0, 1,
  ];
}

export function layoutOcrScene(scene: OcrScene, measure: Measure): { cards: OcrCard[]; unplaced: number[]; height: number } {
  const height = WIDTH / scene.aspect;
  if (!Number.isFinite(height) || height <= 0 || scene.blocks.length > 256) return { cards: [], unplaced: [...new Set(scene.blocks.map(({ id }) => id))], height: WIDTH };
  const scale = (quad: Quad): Quad => quad.map(([x, y]) => [x * WIDTH, y * height]) as Quad;
  const blocks = scene.blocks.map((block) => ({ ...block, quad: scale(block.quad), masks: block.masks.map(scale) }));
  const occupied = [
    ...scene.excluded.map((quad) => ({ id: null, quad: scale(quad) })),
    ...blocks.flatMap((block) => block.masks.map((quad) => ({ id: block.id, quad }))),
  ];
  const cards: OcrCard[] = [];
  const unplaced: number[] = [];
  const pending: typeof blocks = [];
  const free = (quad: Quad, id: number) => {
    const [l, t, r, b] = bounds(quad);
    return l >= 4 && t >= 4 && r <= WIDTH - 4 && b <= height - 4
      && !occupied.some((other) => other.id !== id && overlaps(quad, other.quad));
  };
  const place = (block: typeof blocks[number], quad: Quad, width: number, cardHeight: number, text: NonNullable<ReturnType<typeof fitText>>, connected: boolean) => {
    cards.push({ id: block.id, quad, source: block.quad, width, height: cardHeight, fontSize: text.fontSize, lines: text.lines, connected });
    occupied.push({ id: block.id, quad });
  };
  // Reserve in-place text before looking for extra space around crowded groups.
  for (const block of blocks) {
    const width = (edge(block.quad[0], block.quad[1]) + edge(block.quad[3], block.quad[2])) / 2;
    const cardHeight = (edge(block.quad[0], block.quad[3]) + edge(block.quad[1], block.quad[2])) / 2;
    const text = quadTransform(block.quad, width, cardHeight) && fitText(block.text, width, cardHeight, measure);
    if (text && free(block.quad, block.id)) place(block, block.quad, width, cardHeight, text, false);
    else pending.push(block);
  }
  let connectedCount = 0;
  for (const block of pending) {
    const [l, t, r, b] = bounds(block.quad);
    let placed = false;
    for (const width of [Math.max(r - l, 192), 320, 480, 720]) {
      if (width > WIDTH - 8) continue;
      const text = fitText(block.text, width, Math.min(600, height - 8), measure);
      if (!text) continue;
      const x = (l + r - width) / 2;
      const y = (t + b - text.height) / 2;
      const quad = rectangle([x, y, x + width, y + text.height]);
      if (free(quad, block.id)) {
        place(block, quad, width, text.height, text, false);
        placed = true;
        break;
      }
      if (connectedCount >= 8) continue;
      for (const [x, y] of [[r + 20, (t + b - text.height) / 2], [l - width - 20, (t + b - text.height) / 2], [(l + r - width) / 2, b + 20], [(l + r - width) / 2, t - text.height - 20]]) {
        const card = rectangle([x!, y!, x! + width, y! + text.height]);
        if (!free(card, block.id)) continue;
        place(block, card, width, text.height, text, true);
        connectedCount++;
        placed = true;
        break;
      }
      if (placed) break;
    }
    if (!placed) unplaced.push(block.id);
  }
  return { cards, unplaced: [...new Set(unplaced)], height };
}
