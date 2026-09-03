import { useEffect, useRef, useState } from "react";
import { invoke, Channel } from "@tauri-apps/api/core";
import { useStore } from "../state/store";
import type { Hit, Item, Sort } from "../types";
import { Sparkline, cacheSparks, needsSparks, markPending } from "./Sparkline";

const OVERSCAN = 8;

/** 1x1 transparent PNG; the OS requires a drag image but we want no ghost. */
const DRAG_ICON =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAYAAAAf8/9hAAAAKklEQVR42mNgGAWjYBSMglEwCkbBKBgFo2AUjIJRMApGwSgYBaNgFAxvAAAI8AAB0Y6l0QAAAABJRU5ErkJggg==";

interface Column {
  key: Sort | "wave";
  label: string;
  /** Grid track sizing; the name column takes the slack. */
  width: string;
  align?: "right";
  sortable?: false;
}

const SPARK_W = 96;
const SPARK_H = 18;

const COLUMNS: Column[] = [
  { key: "name", label: "Name", width: "minmax(120px, 2fr)" },
  { key: "wave", label: "Wave", width: `${SPARK_W + 16}px`, sortable: false },
  { key: "folder", label: "Folder", width: "minmax(80px, 1fr)" },
  { key: "duration", label: "Dur", width: "56px", align: "right" },
  { key: "modified", label: "Date", width: "84px", align: "right" },
];

const GRID = COLUMNS.map((c) => c.width).join(" ");

function Highlight({ text, indices }: { text: string; indices: number[] }) {
  if (indices.length === 0) return <>{text}</>;
  const set = new Set(indices);
  return (
    <>
      {Array.from(text).map((ch, i) =>
        set.has(i) ? (
          <b key={i} className="hl">
            {ch}
          </b>
        ) : (
          <span key={i}>{ch}</span>
        ),
      )}
    </>
  );
}

function dur(ms: number) {
  const s = ms / 1000;
  if (s < 60) return `${s.toFixed(2)}`;
  return `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;
}

function date(unix: number) {
  if (!unix) return "—";
  const d = new Date(unix * 1000);
  const now = new Date();
  const sameYear = d.getFullYear() === now.getFullYear();
  return d.toLocaleDateString(undefined, {
    day: "2-digit",
    month: "short",
    ...(sameYear ? {} : { year: "2-digit" }),
  });
}

async function startDrag(id: number, say: (m: string) => void) {
  try {
    const path = await invoke<string>("file_path", { id });
    const onEvent = new Channel();
    // DragItem is an untagged enum: the files variant is a bare array of paths.
    await invoke("plugin:drag|start_drag", {
      item: [path],
      image: DRAG_ICON,
      options: { mode: "copy" },
      onEvent,
    });
  } catch (e) {
    say(`drag failed: ${e}`);
  }
}

function Header() {
  const sort = useStore((s) => s.sort);
  const desc = useStore((s) => s.desc);
  const sortBy = useStore((s) => s.sortBy);
  const query = useStore((s) => s.query);
  const setSort = useStore((s) => s.setSort);

  return (
    <div className="thead" style={{ gridTemplateColumns: GRID }}>
      {COLUMNS.map((c) => (
        <div
          key={c.key}
          className={
            "th" +
            (sort === c.key ? " active" : "") +
            (c.align === "right" ? " r" : "") +
            (c.sortable === false ? " nosort" : "")
          }
          onClick={() => c.sortable !== false && void sortBy(c.key as Sort)}
          title={c.sortable === false ? c.label : `Sort by ${c.label.toLowerCase()}`}
        >
          <span>{c.label}</span>
          {sort === c.key && <span className="arrow">{desc ? "▾" : "▴"}</span>}
        </div>
      ))}
      {sort === "relevance" && query.trim() !== "" && (
        <div className="relevance-tag">by relevance</div>
      )}
      {sort !== "relevance" && query.trim() !== "" && (
        <button
          className="reset-sort"
          title="Back to best-match order"
          onClick={() => void setSort("relevance")}
        >
          ⟲
        </button>
      )}
    </div>
  );
}

export function List() {
  const hits = useStore((s) => s.hits);
  const selected = useStore((s) => s.selected);
  const select = useStore((s) => s.select);
  const say = useStore((s) => s.say);
  const rowH = useStore((s) => s.rowH);

  const ref = useRef<HTMLDivElement>(null);
  const [scroll, setScroll] = useState(0);
  const [height, setHeight] = useState(600);
  const [sparkTick, setSparkTick] = useState(0);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setHeight(el.clientHeight));
    ro.observe(el);
    setHeight(el.clientHeight);
    return () => ro.disconnect();
  }, []);

  // Keep the keyboard selection on screen.
  useEffect(() => {
    const el = ref.current;
    if (!el || selected < 0) return;
    const top = selected * rowH;
    if (top < el.scrollTop) el.scrollTop = top;
    else if (top + rowH > el.scrollTop + el.clientHeight)
      el.scrollTop = top + rowH - el.clientHeight;
  }, [selected, rowH]);

  const first = Math.max(0, Math.floor(scroll / rowH) - OVERSCAN);
  const count = Math.ceil(height / rowH) + OVERSCAN * 2;
  const slice = hits.slice(first, first + count);

  // Fetch peaks only for rows on screen. Scrolling fast would otherwise queue
  // thousands of reads for rows that are already gone.
  useEffect(() => {
    const ids = needsSparks(slice.map(([, item]) => item.id));
    if (ids.length === 0) return;
    markPending(ids);
    const t = setTimeout(() => {
      void invoke<[number, [number, number][]][]>("sparklines", { ids, width: SPARK_W })
        .then((rows) => {
          cacheSparks(rows);
          setSparkTick((n) => n + 1);
        })
        .catch(() => undefined);
    }, 40);
    return () => clearTimeout(t);
  }, [slice.map(([, i]) => i.id).join(",")]);

  return (
    <div className="listwrap">
      <Header />
      <div className="list" ref={ref} onScroll={(e) => setScroll(e.currentTarget.scrollTop)}>
        <div style={{ height: hits.length * rowH, position: "relative" }}>
          {slice.map(([hit, item]: [Hit, Item], n) => {
            const i = first + n;
            return (
              <div
                key={item.id}
                className={"trow" + (i === selected ? " sel" : "")}
                style={{
                  position: "absolute",
                  top: i * rowH,
                  height: rowH,
                  left: 0,
                  right: 0,
                  gridTemplateColumns: GRID,
                }}
                onClick={() => void select(i)}
                draggable
                onDragStart={(e) => {
                  e.preventDefault();
                  void startDrag(item.id, say);
                }}
              >
                <div className="td name" title={item.filename}>
                  <Highlight text={item.filename} indices={hit.indices} />
                </div>
                <div className="td spark-cell">
                  <Sparkline id={item.id} w={SPARK_W} h={SPARK_H} tick={sparkTick} />
                </div>
                <div className={"td" + (hit.via_folder ? " via" : "")} title={item.folder}>
                  {item.folder || "."}
                </div>
                <div className="td r num">{dur(item.duration_ms)}</div>
                <div className="td r num">{date(item.mtime)}</div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
