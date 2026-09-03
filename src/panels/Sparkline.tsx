import { useEffect, useRef } from "react";

const CACHE = new Map<number, [number, number][]>();
const PENDING = new Set<number>();

export function cacheSparks(rows: [number, [number, number][]][]) {
  for (const [id, peaks] of rows) {
    CACHE.set(id, peaks);
    PENDING.delete(id);
  }
}

export function needsSparks(ids: number[]) {
  return ids.filter((id) => !CACHE.has(id) && !PENDING.has(id));
}

export function markPending(ids: number[]) {
  ids.forEach((id) => PENDING.add(id));
}

export function clearSparks() {
  CACHE.clear();
  PENDING.clear();
}

export function Sparkline({ id, w, h, tick }: { id: number; w: number; h: number; tick: number }) {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    const ctx = cv.getContext("2d");
    if (!ctx) return;

    const dpr = devicePixelRatio || 1;
    cv.width = Math.max(1, w * dpr);
    cv.height = Math.max(1, h * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    const peaks = CACHE.get(id);
    if (!peaks || peaks.length === 0) return;

    const mid = h / 2;
    const amp = h / 2 - 1;
    ctx.strokeStyle =
      getComputedStyle(document.documentElement).getPropertyValue("--wave").trim() || "#4ea1ff";
    ctx.globalAlpha = 0.85;
    ctx.beginPath();
    for (let x = 0; x < w; x++) {
      const b0 = Math.floor((x / w) * peaks.length);
      const b1 = Math.max(b0 + 1, Math.floor(((x + 1) / w) * peaks.length));
      let lo = 0;
      let hi = 0;
      for (let b = b0; b < b1 && b < peaks.length; b++) {
        if (peaks[b][0] < lo) lo = peaks[b][0];
        if (peaks[b][1] > hi) hi = peaks[b][1];
      }
      ctx.moveTo(x + 0.5, mid - hi * amp);
      ctx.lineTo(x + 0.5, mid - lo * amp + 0.4);
    }
    ctx.stroke();
    ctx.globalAlpha = 1;
  }, [id, w, h, tick]);

  return <canvas ref={ref} style={{ width: w, height: h }} className="spark" />;
}
