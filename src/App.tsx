import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useStore } from "./state/store";
import { layoutStyle } from "./layout";
import type { Sort } from "./types";
import { Waveform } from "./panels/Waveform";
import { List } from "./panels/List";
import "./app.css";

function Search() {
  const query = useStore((s) => s.query);
  const setQuery = useStore((s) => s.setQuery);
  const addFolder = useStore((s) => s.addFolder);
  const looping = useStore((s) => s.looping);
  const setLooping = useStore((s) => s.setLooping);
  const normalise = useStore((s) => s.normalise);
  const setNormalise = useStore((s) => s.setNormalise);
  const sort = useStore((s) => s.sort);
  const setSort = useStore((s) => s.setSort);
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
      <select
        value={sort}
        title="Sort order"
        onChange={(e) => void setSort(e.target.value as Sort)}
      >
        <option value="relevance">relevance</option>
        <option value="name">name</option>
        <option value="added">recently added</option>
        <option value="modified">file date</option>
        <option value="recent">recently played</option>
        <option value="duration">duration</option>
      </select>
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
          </>
        ) : (
          <span className="dim">select a sound</span>
        )}
      </div>
      <Waveform />
    </div>
  );
}

function Sidebar() {
  const librarySize = useStore((s) => s.librarySize);
  const hits = useStore((s) => s.hits);
  const roots = useStore((s) => s.roots);
  const removeRoot = useStore((s) => s.removeRoot);
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

      <div className="sidebar-h">Folders</div>
      {roots.length === 0 && <div className="dim pad">none yet</div>}
      {roots.map((r) => (
        <div className="root" key={r.id} title={r.path}>
          <span className="root-label">{r.label}</span>
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

function Similar() {
  return (
    <div className="similar">
      <div className="sidebar-h">Similar sounds</div>
      <div className="dim pad">Phase 3</div>
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

  useEffect(() => {
    void useStore.getState().refresh();
    void useStore.getState().loadRoots();
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
      <div style={{ gridArea: "list", minHeight: 0 }}>
        <List />
      </div>
      <div style={{ gridArea: "detail", minHeight: 0, minWidth: 0 }}>
        <Detail />
      </div>
      <div style={{ gridArea: "similar", minHeight: 0 }}>
        <Similar />
      </div>
      <div style={{ gridArea: "status" }}>
        <Status />
      </div>
    </div>
  );
}
