import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useStore } from "./state/store";
import { layoutStyle } from "./layout";
import { Waveform } from "./panels/Waveform";
import { List } from "./panels/List";
import { Similar } from "./panels/Similar";
import { RenameBox, TagEditor, TagRail } from "./panels/Tags";
import { Splitter, useSplitters } from "./panels/Splitter";
import { Updater } from "./panels/Updater";
import { Shortcuts } from "./panels/Shortcuts";
import { ACTIONS, comboOf, resolveTable } from "./actions";
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

  return (
    <div className="search">
      <button onClick={() => void addFolder()}>Add folder…</button>
      <input
        id="search-input"
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
      <Shortcuts />
      <RenameBox />
      <TagEditor />
      <Waveform />
    </div>
  );
}

function Sidebar() {
  const roots = useStore((s) => s.roots);
  const removeRoot = useStore((s) => s.removeRoot);
  const rescanRoot = useStore((s) => s.rescanRoot);

  return (
    <div className="sidebar">
      <Profiles />

      <Failed />
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
    </div>
  );
}

function Failed() {
  const failed = useStore((s) => s.failed);
  const open = useStore((s) => s.showFailed);
  const setOpen = useStore((s) => s.setShowFailed);
  if (failed.length === 0) return null;

  return (
    <>
      <div className="tagrow warn" onClick={() => setOpen(true)}>
        <span>⚠ {failed.length} failed to read</span>
      </div>
      {open && (
        <div className="modal-back" onClick={() => setOpen(false)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-h">
              <b>Files that could not be read</b>
              <span className="spacer" />
              <button onClick={() => setOpen(false)}>Close</button>
            </div>
            <div className="modal-body">
              <div className="pad dim">
                These are indexed but cannot be decoded, so they have no waveform and
                will not play. Usually an unsupported codec or a truncated file.
              </div>
              {failed.map(([name, path]) => (
                <div className="keyrow" key={path} title={path}>
                  <span className="keylabel">{name}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
    </>
  );
}

function Profiles() {
  const reg = useStore((s) => s.registry);
  const s = useStore.getState();
  if (!reg) return null;
  const active = reg.profiles.find((p) => p.id === reg.active);

  return (
    <>
      <div className="sidebar-h">Profile</div>
      <div className="profile-row">
        <span className="dot" style={{ background: active?.color ?? "#888" }} />
        <select
          value={reg.active}
          onChange={(e) => void s.switchProfile(e.target.value)}
          title="Each profile is a separate library with its own folders and tags"
        >
          {reg.profiles.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
      </div>
      <div className="profile-actions">
        <button
          title="Create a new, empty library"
          onClick={() => {
            const name = prompt("Name for the new profile?");
            if (name) void s.createProfile(name);
          }}
        >
          New
        </button>
        <button
          title="Rename this profile"
          onClick={() => {
            const name = prompt("Rename profile", active?.name ?? "");
            if (name && active) void s.renameProfile(active.id, name);
          }}
        >
          Rename
        </button>
        <button
          title="Delete this profile. No audio files are removed."
          disabled={reg.profiles.length < 2}
          onClick={() => {
            if (!active) return;
            if (
              confirm(
                `Delete profile "${active.name}"?\n\nIts folders and tags are forgotten.\nNo audio files are deleted.`,
              )
            )
              void s.deleteProfile(active.id);
          }}
        >
          Delete
        </button>
      </div>
      <div className="profile-actions">
        <button title="Save this profile's tags and favourites to a file" onClick={() => void s.exportPack()}>
          Export tags
        </button>
        <button title="Apply tags and favourites from a pack file" onClick={() => void s.importPack()}>
          Import
        </button>
      </div>
      <div className="profile-actions">
        <button
          title="Delete cached waveforms no profile refers to any more"
          onClick={() => void useStore.getState().pruneCache()}
        >
          Clean cache
        </button>
        <button
          title="Rebindable keyboard shortcuts  ( Ctrl + / )"
          onClick={() => useStore.getState().setShowShortcuts(true)}
        >
          Shortcuts
        </button>
        <button
          title="Check GitHub for a newer version"
          onClick={() => useStore.setState((v) => ({ updateNonce: v.updateNonce + 1 }))}
        >
          Check for updates
        </button>
      </div>
    </>
  );
}

function Status() {
  const nonce = useStore((s) => s.updateNonce);
  const [checked, setChecked] = useState(0);
  const message = useStore((s) => s.message);
  const device = useStore((s) => s.device);
  const scanning = useStore((s) => s.scanning);
  return (
    <div className="status">
      <Updater manual={nonce > checked} onDone={() => setChecked(nonce)} />
      <span>{scanning ? `scanning ${scanning.done}/${scanning.total}` : message}</span>
      {scanning && (
        <button className="x" title="Stop scanning" onClick={() => useStore.getState().cancelScan()}>
          stop
        </button>
      )}
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
    void useStore.getState().loadProfiles();
    void useStore.getState().loadTags();
    void useStore.getState().loadFailed();
    // The store restored these from disk; the audio thread has not seen them.
    void invoke("set_volume", { volume: useStore.getState().volume });
    void invoke("set_looping", { looping: useStore.getState().looping });
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

  const keymap = useStore((s) => s.keymap);
  useEffect(() => {
    const table = resolveTable(keymap);

    const run = (id: string) => {
      const s = useStore.getState();
      const item = s.currentItem();
      switch (id) {
        case "search.focus":
          document.getElementById("search-input")?.focus();
          return true;
        case "search.clear":
          s.setQuery("");
          return true;
        case "transport.playPause":
          s.toggle();
          return true;
        case "transport.playRegion":
          if (s.region) s.playRegion(s.region);
          return true;
        case "transport.loop":
          s.setLooping(!s.looping);
          return true;
        case "nav.up":
          void s.move(-1);
          return true;
        case "nav.down":
          void s.move(1);
          return true;
        case "region.in":
          s.markIn();
          return true;
        case "region.out":
          s.markOut();
          return true;
        case "zoom.in":
          s.zoom(0.5);
          return true;
        case "zoom.out":
          s.zoom(2);
          return true;
        case "zoom.fit":
          s.zoomToFit();
          return true;
        case "zoom.region":
          s.zoomToRegion();
          return true;
        case "organise.favorite":
          if (item) void s.toggleFavorite(item.id);
          return true;
        case "organise.tag":
          if (item) document.getElementById("tag-input")?.focus();
          return true;
        case "organise.rename":
          if (item) s.beginRename();
          return true;
        case "organise.undo":
          void s.undoRename();
          return true;
        case "app.shortcuts":
          s.setShowShortcuts(!s.showShortcuts);
          return true;
      }
      return false;
    };

    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null;
      const typing = el?.tagName === "INPUT" || el?.tagName === "TEXTAREA";
      const id = table.get(comboOf(e));
      if (!id) return;

      // While typing, only actions declared for the search context fire, so
      // ordinary letters are never swallowed.
      const def = ACTIONS.find((a) => a.id === id);
      if (typing && def?.context !== "search") return;

      if (run(id)) e.preventDefault();
    };

    addEventListener("keydown", onKey);
    return () => removeEventListener("keydown", onKey);
  }, [keymap]);

  return (
    <div style={layoutStyle(layout)}>
      <div style={{ gridArea: "search" }}>
        <Search />
      </div>
      <div style={{ gridArea: "sidebar", minHeight: 0 }}>
        <Sidebar />
      </div>
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
