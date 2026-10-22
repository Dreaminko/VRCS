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
