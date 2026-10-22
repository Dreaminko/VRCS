export function SurfacePointer({ point }: { point: [number, number] | null }) {
  return point && <span className="vr-surface-pointer" aria-hidden="true" style={{ left: `${point[0] * 100}%`, top: `${point[1] * 100}%` }} />;
}
