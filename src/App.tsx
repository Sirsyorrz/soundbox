import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useStore } from "./state/store";
import { layoutStyle, LAYOUTS, type LayoutName } from "./layout";
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
  const layout = useStore((s) => s.layout);
  const setLayout = useStore((s) => s.setLayout);
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
      <select value={layout} onChange={(e) => setLayout(e.target.value as LayoutName)}>
        {Object.keys(LAYOUTS).map((k) => (
          <option key={k} value={k}>
            {k}
          </option>
        ))}
      </select>
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
  return (
    <div className="sidebar">
      <div className="sidebar-h">Library</div>
      <div className="stat">
        <b>{librarySize.toLocaleString()}</b> sounds
      </div>
      <div className="stat">
        <b>{hits.length.toLocaleString()}</b> shown
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
