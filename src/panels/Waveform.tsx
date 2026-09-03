import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useStore } from "../state/store";

const MIN_VIEW_FRAMES = 256;

function css(name: string, fallback: string) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
}

export function Waveform() {
  const current = useStore((s) => s.current);
  const region = useStore((s) => s.region);
  const view = useStore((s) => s.view);
  const setView = useStore((s) => s.setView);
  const pos = useStore((s) => s.pos);
  const playing = useStore((s) => s.playing);
  const setRegion = useStore((s) => s.setRegion);
  const playRegion = useStore((s) => s.playRegion);

  const ref = useRef<HTMLCanvasElement>(null);
  const drag = useRef<number | null>(null);
  const [peaks, setPeaks] = useState<[number, number][][]>([]);

  // Peaks are fetched for the visible range only, so zooming in gains real
  // detail instead of stretching the full-file summary.
  useEffect(() => {
    if (!current || !view) {
      setPeaks([]);
      return;
    }
    const width = Math.max(64, Math.round(ref.current?.clientWidth ?? 800));
    let stale = false;
    const t = setTimeout(() => {
      void invoke<[number, number][][]>("peaks_range", {
        start: view[0],
        end: view[1],
        width,
      }).then((p) => {
        if (!stale) setPeaks(p);
      });
    }, 16);
    return () => {
      stale = true;
      clearTimeout(t);
    };
  }, [current, view]);

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
    if (!current || !view || peaks.length === 0) return;

    const [vs, ve] = view;
    const span = Math.max(1, ve - vs);
    const toX = (f: number) => ((f - vs) / span) * w;

    if (region) {
      ctx.fillStyle = css("--region", "rgba(255,204,78,0.12)");
      const x0 = toX(region[0]);
      const x1 = toX(region[1]);
      ctx.fillRect(x0, 0, Math.max(1, x1 - x0), h);
    }

    const lanes = peaks.length;
    const laneH = h / lanes;
    const axis = css("--wave-axis", "#2a3441");
    const wave = css("--wave", "#4ea1ff");

    peaks.forEach((pk, c) => {
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

    if (region) {
      ctx.strokeStyle = css("--sel", "#ffcc4e");
      ctx.globalAlpha = 0.55;
      for (const f of region) {
        const x = toX(f);
        ctx.beginPath();
        ctx.moveTo(x, 0);
        ctx.lineTo(x, h);
        ctx.stroke();
      }
      ctx.globalAlpha = 1;
    }

    if (playing) {
      const x = toX(pos);
      if (x >= 0 && x <= w) {
        ctx.strokeStyle = css("--sel", "#ffcc4e");
        ctx.lineWidth = 1.5;
        ctx.beginPath();
        ctx.moveTo(x, 0);
        ctx.lineTo(x, h);
        ctx.stroke();
        ctx.lineWidth = 1;
      }
    }

    if (current.frames > 0 && span < current.frames) {
      // Minimap of where the view sits in the whole file.
      const bh = 3;
      ctx.fillStyle = css("--line", "#232b36");
      ctx.fillRect(0, h - bh, w, bh);
      ctx.fillStyle = css("--accent", "#4ea1ff");
      ctx.fillRect((vs / current.frames) * w, h - bh, Math.max(2, (span / current.frames) * w), bh);
    }
  }, [current, region, pos, playing, peaks, view]);

  const frameAt = (e: React.MouseEvent) => {
    if (!current || !ref.current || !view) return 0;
    const r = ref.current.getBoundingClientRect();
    const t = (e.clientX - r.left) / r.width;
    return Math.round(view[0] + Math.max(0, Math.min(1, t)) * (view[1] - view[0]));
  };

  return (
    <canvas
      ref={ref}
      className="wave"
      onWheel={(e) => {
        if (!current || !view) return;
        const r = ref.current!.getBoundingClientRect();
        const t = (e.clientX - r.left) / r.width;
        const anchor = view[0] + t * (view[1] - view[0]);
        const factor = e.deltaY > 0 ? 1.25 : 0.8;
        const span = Math.min(
          current.frames,
          Math.max(MIN_VIEW_FRAMES, Math.round((view[1] - view[0]) * factor)),
        );
        // Keep the frame under the cursor fixed while zooming.
        let start = Math.round(anchor - t * span);
        start = Math.max(0, Math.min(current.frames - span, start));
        setView([start, start + span]);
      }}
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
        // A click seeks and plays to the end of the view; a drag defines a region.
        const r: [number, number] =
          Math.abs(f - drag.current) < 3
            ? [f, view ? view[1] : current.frames]
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
