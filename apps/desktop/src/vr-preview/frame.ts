export interface PreviewFrame {
  width: number;
  height: number;
  generation: number;
  revision: number;
  panel: number;
  sequence: number;
  pixels: Uint8ClampedArray;
}

export function decodeFrame(buffer: ArrayBuffer): PreviewFrame {
  if (buffer.byteLength < 40) throw new Error("Invalid preview frame");
  const view = new DataView(buffer);
  const fields = Array.from({ length: 10 }, (_, i) => view.getUint32(i * 4, true));
  const [magic, version, width, height, format, alpha, generation, revision, panel, sequence] = fields;
  if (magic !== 0x56524353 || version !== 1 || width < 1 || height < 1 || width > 4096 || height > 4096
    || ![1, 2].includes(format) || ![1, 2].includes(alpha) || buffer.byteLength !== 40 + width * height * 4) {
    throw new Error("Invalid preview frame");
  }
  const pixels = new Uint8ClampedArray(buffer, 40);
  for (let i = 0; i < pixels.length; i += 4) {
    if (format === 2) [pixels[i], pixels[i + 2]] = [pixels[i + 2], pixels[i]];
    if (alpha === 2) {
      const a = pixels[i + 3];
      for (let c = 0; c < 3; c++) pixels[i + c] = a ? Math.round(pixels[i + c] * 255 / a) : 0;
    }
  }
  return { width, height, generation, revision, panel, sequence, pixels };
}

export function texturePoint(x: number, y: number, box: { width: number; height: number }, texture: { width: number; height: number }): [number, number] | null {
  if (![x, y, box.width, box.height, texture.width, texture.height].every(Number.isFinite)
    || Math.min(box.width, box.height, texture.width, texture.height) <= 0) return null;
  const scale = Math.min(box.width / texture.width, box.height / texture.height);
  const width = texture.width * scale;
  const height = texture.height * scale;
  const u = (x - (box.width - width) / 2) / width;
  const v = (y - (box.height - height) / 2) / height;
  return u >= 0 && u < 1 && v >= 0 && v < 1 ? [u, v] : null;
}
