import { useEffect } from "react";

type Axis = "x" | "y";

interface Bounds {
  min: number;
  /** Space that must remain for the panels on the other side. */
  reserve: number;
}

const LIMITS: Record<string, Bounds> = {
  "--sidebar-w": { min: 120, reserve: 420 },
  "--list-w": { min: 220, reserve: 320 },
  "--similar-h": { min: 60, reserve: 220 },
};

const STORE_KEY = "soundbox.layout.sizes";

function read(name: string) {
  return parseFloat(getComputedStyle(document.documentElement).getPropertyValue(name)) || 0;
}

function clamp(name: string, px: number) {
  const b = LIMITS[name];
  if (!b) return px;
  const axisSize = name === "--similar-h" ? innerHeight : innerWidth;
  const others =
    name === "--list-w" ? read("--sidebar-w") : name === "--sidebar-w" ? read("--list-w") : 0;
  const max = Math.max(b.min, axisSize - others - b.reserve);
  return Math.round(Math.min(max, Math.max(b.min, px)));
}

function apply(name: string, px: number) {
  document.documentElement.style.setProperty(name, `${clamp(name, px)}px`);
}

function persist() {
  const sizes = Object.keys(LIMITS).reduce<Record<string, number>>((acc, k) => {
    acc[k] = read(k);
    return acc;
  }, {});
  localStorage.setItem(STORE_KEY, JSON.stringify(sizes));
}

/** Restores saved sizes and keeps them legal as the window changes size. */
export function useSplitters() {
  useEffect(() => {
    try {
      const saved = JSON.parse(localStorage.getItem(STORE_KEY) ?? "{}");
      for (const [k, v] of Object.entries(saved)) {
        if (typeof v === "number") apply(k, v);
      }
    } catch {
      // A corrupt entry should not stop the app rendering.
    }
    // Shrinking the window can leave a panel wider than the window itself.
    const onResize = () => Object.keys(LIMITS).forEach((k) => apply(k, read(k)));
    addEventListener("resize", onResize);
    return () => removeEventListener("resize", onResize);
  }, []);
}

export function Splitter({
  axis,
  varName,
  invert,
  area,
}: {
  axis: Axis;
  varName: string;
  /** Dragging up/left grows the panel rather than shrinking it. */
  invert?: boolean;
  area?: string;
}) {
  return (
    <div
      className={`splitter ${axis}`}
      style={area ? { gridArea: area } : undefined}
      onDoubleClick={() => {
        apply(varName, LIMITS[varName]?.min ?? 0);
        persist();
      }}
      onPointerDown={(e) => {
        e.preventDefault();
        const el = e.currentTarget;
        el.setPointerCapture(e.pointerId);
        const startPos = axis === "x" ? e.clientX : e.clientY;
        const startSize = read(varName);
        document.body.classList.add(axis === "x" ? "col-resize" : "row-resize");

        const move = (ev: PointerEvent) => {
          const delta = (axis === "x" ? ev.clientX : ev.clientY) - startPos;
          apply(varName, startSize + (invert ? -delta : delta));
        };
        const up = () => {
          el.releasePointerCapture(e.pointerId);
          el.removeEventListener("pointermove", move);
          el.removeEventListener("pointerup", up);
          document.body.classList.remove("col-resize", "row-resize");
          persist();
        };
        el.addEventListener("pointermove", move);
        el.addEventListener("pointerup", up);
      }}
    />
  );
}
