import { useEffect, useRef, useState } from "react";
import { invoke, Channel } from "@tauri-apps/api/core";
import { useStore } from "../state/store";
import type { Hit, Item } from "../types";

const ROW_H = 44;
const OVERSCAN = 8;

/** 1x1 transparent PNG; the OS needs some drag image but we want no ghost. */
const DRAG_ICON =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAYAAAAf8/9hAAAAKklEQVR42mNgGAWjYBSMglEwCkbBKBgFo2AUjIJRMApGwSgYBaNgFAxvAAAI8AAB0Y6l0QAAAABJRU5ErkJggg==";

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
  return s < 60 ? `${s.toFixed(2)}s` : `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;
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

export function List() {
  const hits = useStore((s) => s.hits);
  const selected = useStore((s) => s.selected);
  const select = useStore((s) => s.select);
  const say = useStore((s) => s.say);

  const ref = useRef<HTMLDivElement>(null);
  const [scroll, setScroll] = useState(0);
  const [height, setHeight] = useState(600);

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
    const top = selected * ROW_H;
    if (top < el.scrollTop) el.scrollTop = top;
    else if (top + ROW_H > el.scrollTop + el.clientHeight)
      el.scrollTop = top + ROW_H - el.clientHeight;
  }, [selected]);

  const first = Math.max(0, Math.floor(scroll / ROW_H) - OVERSCAN);
  const count = Math.ceil(height / ROW_H) + OVERSCAN * 2;
  const slice = hits.slice(first, first + count);

  return (
    <div className="list" ref={ref} onScroll={(e) => setScroll(e.currentTarget.scrollTop)}>
      <div style={{ height: hits.length * ROW_H, position: "relative" }}>
        {slice.map(([hit, item]: [Hit, Item], n) => {
          const i = first + n;
          return (
            <div
              key={item.id}
              className={"row" + (i === selected ? " sel" : "")}
              style={{ position: "absolute", top: i * ROW_H, height: ROW_H, left: 0, right: 0 }}
              onClick={() => void select(i)}
              draggable
              onDragStart={(e) => {
                e.preventDefault();
                void startDrag(item.id, say);
              }}
            >
              <div className="name">
                <Highlight text={item.filename} indices={hit.indices} />
              </div>
              <div className="meta">
                <span className={hit.via_folder ? "folder via" : "folder"}>
                  {item.folder || "."}
                </span>
                <span className="spacer" />
                <span>{item.channels === 1 ? "mono" : item.channels === 2 ? "stereo" : `${item.channels}ch`}</span>
                <span>{dur(item.duration_ms)}</span>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
