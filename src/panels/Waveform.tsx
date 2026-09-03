import { useEffect, useRef } from "react";
import { useStore } from "../state/store";

function css(name: string, fallback: string) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
}

export function Waveform() {
  const current = useStore((s) => s.current);
  const region = useStore((s) => s.region);
  const pos = useStore((s) => s.pos);
  const playing = useStore((s) => s.playing);
  const setRegion = useStore((s) => s.setRegion);
  const playRegion = useStore((s) => s.playRegion);

  const ref = useRef<HTMLCanvasElement>(null);
  const drag = useRef<number | null>(null);

  useEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    const ctx = cv.getContext("2d");
    if (!ctx) return;

    const dpr = devicePixelRatio || 1;
    const w = cv.clientWidth;
    const h = cv.clientHeight;
    cv.width = Math.max(1, Math.floor(w * dpr));
    cv.height = Math.max(1, Math.floor(h * dpr));
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    if (!current || current.peaks.length === 0) return;

    if (region) {
      ctx.fillStyle = css("--region", "rgba(255,204,78,0.12)");
      const x0 = (region[0] / current.frames) * w;
      const x1 = (region[1] / current.frames) * w;
      ctx.fillRect(x0, 0, Math.max(1, x1 - x0), h);
    }

    // One lane per channel: mono draws a single waveform, stereo stacks L/R.
    const lanes = current.peaks.length;
    const laneH = h / lanes;
    const axis = css("--wave-axis", "#2a3441");
    const wave = css("--wave", "#4ea1ff");

    current.peaks.forEach((pk, c) => {
      const top = c * laneH;
      const mid = top + laneH / 2;
      const amp = laneH / 2 - 3;

      ctx.strokeStyle = axis;
      ctx.beginPath();
      ctx.moveTo(0, mid);
      ctx.lineTo(w, mid);
      ctx.stroke();

      ctx.strokeStyle = wave;
      ctx.beginPath();
      for (let x = 0; x < w; x++) {
        const b0 = Math.floor((x / w) * pk.length);
        const b1 = Math.max(b0 + 1, Math.floor(((x + 1) / w) * pk.length));
        let lo = 0;
        let hi = 0;
        // Aggregate every bucket under this pixel, otherwise zoomed-out views
        // sample sparsely and transients disappear.
        for (let b = b0; b < b1 && b < pk.length; b++) {
          if (pk[b][0] < lo) lo = pk[b][0];
          if (pk[b][1] > hi) hi = pk[b][1];
        }
        ctx.moveTo(x + 0.5, mid - hi * amp);
        ctx.lineTo(x + 0.5, mid - lo * amp + 0.5);
      }
      ctx.stroke();

      if (c > 0) {
        ctx.strokeStyle = css("--line", "#232b36");
        ctx.beginPath();
        ctx.moveTo(0, top);
        ctx.lineTo(w, top);
        ctx.stroke();
      }
    });

    if (playing) {
      const x = (pos / current.frames) * w;
      ctx.strokeStyle = css("--sel", "#ffcc4e");
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, h);
      ctx.stroke();
      ctx.lineWidth = 1;
    }
  }, [current, region, pos, playing]);

  useEffect(() => {
    const onResize = () => useStore.setState({});
    addEventListener("resize", onResize);
    return () => removeEventListener("resize", onResize);
  }, []);

  const frameAt = (e: React.MouseEvent) => {
    if (!current || !ref.current) return 0;
    const r = ref.current.getBoundingClientRect();
    const t = (e.clientX - r.left) / r.width;
    return Math.round(Math.max(0, Math.min(1, t)) * current.frames);
  };

  return (
    <canvas
      ref={ref}
      className="wave"
      onMouseDown={(e) => {
        if (current) drag.current = frameAt(e);
      }}
      onMouseMove={(e) => {
        if (drag.current == null || !current) return;
        const f = frameAt(e);
        setRegion([Math.min(drag.current, f), Math.max(drag.current, f)]);
      }}
      onMouseUp={(e) => {
        if (drag.current == null || !current) return;
        const f = frameAt(e);
        // A click seeks and plays to the end; a drag defines a region.
        const r: [number, number] =
          Math.abs(f - drag.current) < 3
            ? [f, current.frames]
            : [Math.min(drag.current, f), Math.max(drag.current, f)];
        drag.current = null;
        playRegion(r);
      }}
      onMouseLeave={() => {
        drag.current = null;
      }}
    />
  );
}
