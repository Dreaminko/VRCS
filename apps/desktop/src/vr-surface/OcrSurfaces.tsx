import { useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { layoutOcrScene, memoizeTextMeasure, quadTransform, type OcrScene, type Quad } from "./ocr-scene";
import "./ocr-surface.css";

export function OcrSelectionSurface() {
  return <div className="vr-ocr-selection" />;
}

export function OcrResultSurface({ scene }: { scene: OcrScene }) {
  const root = useRef<HTMLDivElement>(null);
  const context = useRef<CanvasRenderingContext2D | null>(null);
  const measurement = useRef<{ family: string; measure: ReturnType<typeof memoizeTextMeasure> } | null>(null);
  const [layout, setLayout] = useState<{ scene: OcrScene; result: ReturnType<typeof layoutOcrScene> } | null>(null);
  const current = layout?.scene === scene ? layout.result : null;
  useLayoutEffect(() => {
    let cancelled = false;
    const measure = () => {
      const element = root.current;
      if (!element || cancelled) return;
      const canvas = context.current ??= document.createElement("canvas").getContext("2d");
      if (!canvas) return;
      const family = getComputedStyle(element).fontFamily;
      if (measurement.current?.family !== family) {
        measurement.current = { family, measure: memoizeTextMeasure((text, size) => {
          const font = `600 ${size}px ${family}`;
          if (canvas.font !== font) canvas.font = font;
          return canvas.measureText(text).width;
        }) };
      }
      const result = layoutOcrScene(scene, measurement.current.measure);
      setLayout({ scene, result });
    };
    const fontsChanged = () => { measurement.current = null; measure(); };
    document.fonts.addEventListener("loadingdone", fontsChanged);
    void document.fonts.ready.then(measure);
    return () => { cancelled = true; document.fonts.removeEventListener("loadingdone", fontsChanged); };
  }, [scene]);
  useLayoutEffect(() => {
    if (current) root.current?.dispatchEvent(new CustomEvent("vr-surface-layout", { bubbles: true }));
  }, [current]);
  const height = current?.height ?? 2048 / scene.aspect;
  const scaleQuad = (quad: Quad) => quad.map(([x, y]) => `${x * 2048},${y * height}`).join(" ");
  const placed = new Set(current?.cards.map(({ id }) => id));
  return <div ref={root} className="vr-ocr-result" data-vr-pending={current ? undefined : "true"}
    data-vr-unplaced={JSON.stringify(current?.unplaced ?? [])}
    data-vr-capture-bounds={current ? JSON.stringify(current.captureBounds) : undefined}
    style={{ "--vr-background-opacity": scene.opacity } as CSSProperties}>
    <div className="vr-ocr-canvas" style={{ height, transform: `scaleY(${scene.aspect})` }}>
      <svg className="vr-ocr-masks" viewBox={`0 0 2048 ${height}`} aria-hidden="true">
        {scene.blocks.filter(({ id }) => placed.has(id)).flatMap((block) => block.masks.map((mask, index) => <polygon key={`${block.id}-${index}`} points={scaleQuad(mask)} />))}
        {current?.cards.filter(({ connected }) => connected).map((card) => {
          const center = (quad: Quad) => [quad.reduce((sum, p) => sum + p[0], 0) / 4, quad.reduce((sum, p) => sum + p[1], 0) / 4];
          const [x1, y1] = center(card.source);
          const [x2, y2] = center(card.quad);
          return <line key={card.id} x1={x1} y1={y1} x2={x2} y2={y2} />;
        })}
      </svg>
      {current?.cards.map((card) => {
        const matrix = quadTransform(card.quad, card.width, card.height)!;
        return <div key={card.id} className="vr-ocr-card vr-surface-panel" style={{ width: card.width, height: card.height, fontSize: card.fontSize, transform: `matrix3d(${matrix.join(",")})` }}>
          {card.lines.join("\n")}
        </div>;
      })}
    </div>
  </div>;
}
