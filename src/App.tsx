import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useStore } from "./state/store";
import { layoutStyle } from "./layout";
import { Waveform } from "./panels/Waveform";
import { List } from "./panels/List";
import { Similar } from "./panels/Similar";
import { RenameBox, TagEditor, TagRail } from "./panels/Tags";
import { Splitter, useSplitters } from "./panels/Splitter";
import "./app.css";

function Search() {
  const query = useStore((s) => s.query);
  const setQuery = useStore((s) => s.setQuery);
  const addFolder = useStore((s) => s.addFolder);
  const looping = useStore((s) => s.looping);
  const setLooping = useStore((s) => s.setLooping);
  const normalise = useStore((s) => s.normalise);
  const setNormalise = useStore((s) => s.setNormalise);
  const volume = useStore((s) => s.volume);
  const setVolume = useStore((s) => s.setVolume);
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const f = (e: KeyboardEvent) => {
      if (e.key === "/" && document.activeElement?.tagName !== "INPUT") {
        e.preventDefault();
        ref.current?.focus();
      }
    };
    addEventListener("keydown", f);
    return () => removeEventListener("keydown", f);
  }, []);

  return (
    <div className="search">
      <button onClick={() => void addFolder()}>Add folder…</button>
      <input
        ref={ref}
        value={query}
        placeholder="search…   ( / to focus )"
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") setQuery("");
        }}
      />
      <label>
        <input type="checkbox" checked={looping} onChange={(e) => setLooping(e.target.checked)} />
        loop
      </label>
      <label title="Match preview level to -18 LUFS">
        <input
          type="checkbox"
          checked={normalise}
          onChange={(e) => void setNormalise(e.target.checked)}
        />
        normalise
      </label>
      <label className="vol" title={`Volume ${Math.round(volume * 100)}%`}>
        vol
        <input
          type="range"
          min={0}
          max={1.5}
          step={0.01}
          value={volume}
          onChange={(e) => setVolume(Number(e.target.value))}
        />
      </label>
    </div>
  );
}

function Zoom() {
  const view = useStore((s) => s.view);
  const current = useStore((s) => s.current);
  const zoom = useStore((s) => s.zoom);
  const fit = useStore((s) => s.zoomToFit);
  if (!current || !view) return null;
  const pct = (current.frames / Math.max(1, view[1] - view[0])).toFixed(1);
  return (
    <span className="zoom">
      <button onClick={() => zoom(2)} title="Zoom out  ( - )">
        −
      </button>
      <button onClick={() => zoom(0.5)} title="Zoom in  ( + )">
        +
      </button>
      <button onClick={fit} title="Fit whole file  ( 0 )">
        fit
      </button>
      <span className="dim">{pct}x</span>
    </span>
  );
}

function Detail() {
  const current = useStore((s) => s.current);
  const hits = useStore((s) => s.hits);
  const selected = useStore((s) => s.selected);
  const item = hits[selected]?.[1];

  return (
    <div className="detail">
      <div className="detail-meta">
        {current && item ? (
          <>
            <span className="title">{item.filename}</span>
            <span className="spacer" />
            <span>{current.sample_rate} Hz</span>
            <span>{current.channels === 1 ? "mono" : `${current.channels} ch`}</span>
            <span>{(current.duration_ms / 1000).toFixed(2)} s</span>
            <span>{current.lufs != null ? `${current.lufs.toFixed(1)} LUFS` : "LUFS n/a"}</span>
            <Zoom />
          </>
        ) : (
          <span className="dim">select a sound</span>
        )}
      </div>
      <RenameBox />
      <TagEditor />
      <Waveform />
    </div>
  );
}

function Sidebar() {
  const librarySize = useStore((s) => s.librarySize);
  const hits = useStore((s) => s.hits);
  const roots = useStore((s) => s.roots);
  const removeRoot = useStore((s) => s.removeRoot);
  const rescanRoot = useStore((s) => s.rescanRoot);
  const addFolder = useStore((s) => s.addFolder);

  return (
    <div className="sidebar">
      <div className="sidebar-h">Library</div>
      <div className="stat">
        <b>{librarySize.toLocaleString()}</b> sounds
      </div>
      <div className="stat">
        <b>{hits.length.toLocaleString()}</b> shown
      </div>

      <TagRail />

      <div className="sidebar-h">Folders</div>
      {roots.length === 0 && <div className="dim pad">none yet</div>}
      {roots.map((r) => (
        <div className="root" key={r.id} title={r.path}>
          <span className="root-label">{r.label}</span>
          <button
            className="x"
            title="Rescan this folder for new or changed sounds"
            onClick={() => void rescanRoot(r.id)}
          >
            ⟳
          </button>
          <button
            className="x"
            title={`Remove ${r.path} from the library.\nFiles on disk are not touched.`}
            onClick={() => {
              if (confirm(`Remove "${r.label}" from the library?\n\nNo files on disk are deleted.`))
                void removeRoot(r.id);
            }}
          >
            ×
          </button>
        </div>
      ))}
      <div className="pad">
        <button onClick={() => void addFolder()}>Add folder…</button>
      </div>
    </div>
  );
}

function Status() {
  const message = useStore((s) => s.message);
  const device = useStore((s) => s.device);
  const scanning = useStore((s) => s.scanning);
  return (
    <div className="status">
      <span>{scanning ? `scanning ${scanning.done}/${scanning.total}` : message}</span>
      <span className="spacer" />
      <span className="dim">{device}</span>
    </div>
  );
}

export default function App() {
  const layout = useStore((s) => s.layout);
  useSplitters();

  useEffect(() => {
    void useStore.getState().refresh();
    void useStore.getState().loadRoots();
    void useStore.getState().loadTags();
    void invoke<string>("device_info").then((device) => useStore.setState({ device }));

    const un = listen<{ done: number; total: number }>("scan:progress", (e) =>
      useStore.setState({ scanning: e.payload }),
    );

    const id = setInterval(() => void useStore.getState().tick(), 33);
    return () => {
      clearInterval(id);
      void un.then((f) => f());
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const inInput = (e.target as HTMLElement)?.tagName === "INPUT";
      const s = useStore.getState();
      if (e.code === "Space" && !inInput) {
        e.preventDefault();
        s.toggle();
      }
      if (e.code === "ArrowDown" && !inInput) {
        e.preventDefault();
        void s.move(1);
      }
      if (e.code === "ArrowUp" && !inInput) {
        e.preventDefault();
        void s.move(-1);
      }
      if (e.code === "KeyL" && !inInput) s.setLooping(!s.looping);
      if (inInput) return;
      if (e.code === "BracketLeft") s.markIn();
      if (e.code === "BracketRight") s.markOut();
      if (e.code === "Equal" || e.code === "NumpadAdd") s.zoom(0.5);
      if (e.code === "Minus" || e.code === "NumpadSubtract") s.zoom(2);
      if (e.code === "Digit0" || e.code === "Numpad0") s.zoomToFit();
      if (e.code === "KeyZ" && !e.ctrlKey) s.zoomToRegion();
      if (e.code === "Enter" && s.region) s.playRegion(s.region);
      const item = s.currentItem();
      if (e.code === "KeyF" && item) void s.toggleFavorite(item.id);
      if (e.code === "F2" && item) {
        e.preventDefault();
        s.beginRename();
      }
      if (e.code === "KeyZ" && e.ctrlKey) {
        e.preventDefault();
        void s.undoRename();
      }
      if (e.code === "KeyT" && item) {
        e.preventDefault();
        document.getElementById("tag-input")?.focus();
      }
    };
    addEventListener("keydown", onKey);
    return () => removeEventListener("keydown", onKey);
  }, []);

  return (
    <div style={layoutStyle(layout)}>
      <div style={{ gridArea: "search" }}>
        <Search />
      </div>
      <div style={{ gridArea: "sidebar", minHeight: 0 }}>
        <Sidebar />
      </div>
      <Splitter axis="x" varName="--sidebar-w" area="gutA" />
      <div style={{ gridArea: "list", minHeight: 0, minWidth: 0 }}>
        <List />
      </div>
      <Splitter axis="x" varName="--list-w" area="gutB" />
      <div className="rightcol" style={{ gridArea: "right" }}>
        <Detail />
        <Splitter axis="y" varName="--similar-h" invert />
        <Similar />
      </div>
      <div style={{ gridArea: "status" }}>
        <Status />
      </div>
    </div>
  );
}
