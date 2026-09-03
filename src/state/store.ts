import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { Filter, Hit, Item, Loaded, PackPreview, Registry, Root, Sort } from "../types";
import { clearSparks } from "../panels/Sparkline";
import { loadSettings, saveSettings } from "../settings";
import type { Keymap } from "../actions";
import type { LayoutName } from "../layout";

const SEARCH_LIMIT = 500;

interface State {
  query: string;
  hits: [Hit, Item][];
  selected: number;
  current: Loaded | null;
  region: [number, number] | null;
  /** Visible frame range of the detail waveform. */
  view: [number, number] | null;
  playing: boolean;
  pos: number;
  looping: boolean;
  normalise: boolean;
  volume: number;
  sort: Sort;
  desc: boolean;
  rowH: number;
  similarGate: boolean;
  roots: Root[];
  tags: [string, number][];
  filter: Filter;
  librarySize: number;
  scanning: { done: number; total: number } | null;
  device: string;
  layout: LayoutName;
  message: string;

  setQuery: (q: string) => void;
  refresh: () => Promise<void>;
  select: (i: number) => Promise<void>;
  selectById: (id: number) => Promise<void>;
  setSimilarGate: (v: boolean) => void;
  currentItem: () => Item | null;
  renaming: boolean;
  beginRename: () => void;
  cancelRename: () => void;
  commitRename: (stem: string) => Promise<void>;
  undoRename: () => Promise<void>;
  toggleFavorite: (id: number) => Promise<void>;
  addTag: (id: number, tag: string) => Promise<void>;
  removeTag: (id: number, tag: string) => Promise<void>;
  loadTags: () => Promise<void>;
  setFilter: (f: Filter) => Promise<void>;
  move: (delta: number) => Promise<void>;
  setRegion: (r: [number, number] | null) => void;
  setView: (v: [number, number]) => void;
  zoom: (factor: number) => void;
  zoomToFit: () => void;
  zoomToRegion: () => void;
  markIn: () => void;
  markOut: () => void;
  playRegion: (r?: [number, number]) => void;
  toggle: () => void;
  stop: () => void;
  setLooping: (v: boolean) => void;
  setNormalise: (v: boolean) => Promise<void>;
  setVolume: (v: number) => void;
  setSort: (s: Sort) => Promise<void>;
  sortBy: (s: Sort) => Promise<void>;
  loadRoots: () => Promise<void>;
  removeRoot: (id: number) => Promise<void>;
  rescanRoot: (id: number) => Promise<void>;
  setLayout: (l: LayoutName) => void;
  addFolder: () => Promise<void>;
  registry: Registry | null;
  updateNonce: number;
  keymap: Keymap;
  setBinding: (id: string, combos: string[]) => void;
  resetBinding: (id: string) => void;
  resetBindings: () => void;
  showShortcuts: boolean;
  setShowShortcuts: (v: boolean) => void;
  loadProfiles: () => Promise<void>;
  switchProfile: (id: string) => Promise<void>;
  createProfile: (name: string) => Promise<void>;
  renameProfile: (id: string, name: string) => Promise<void>;
  deleteProfile: (id: string) => Promise<void>;
  exportPack: () => Promise<void>;
  importPack: (path?: string) => Promise<void>;
  tick: () => Promise<void>;
  say: (m: string) => void;
}

const SAVED = loadSettings();

export const useStore = create<State>((set, get) => ({
  query: "",
  hits: [],
  selected: -1,
  current: null,
  region: null,
  view: null,
  playing: false,
  pos: 0,
  looping: SAVED.looping,
  normalise: SAVED.normalise,
  volume: SAVED.volume,
  registry: null,
  updateNonce: 0,
  keymap: SAVED.keymap,
  showShortcuts: false,
  sort: SAVED.sort,
  desc: SAVED.desc,
  rowH: 26,
  similarGate: true,
  roots: [],
  tags: [],
  filter: { favoritesOnly: false, tag: null },
  renaming: false,
  librarySize: 0,
  scanning: null,
  device: "",
  layout: "default",
  message: "",

  say: (message) => set({ message }),

  setQuery: (query) => {
    set({ query });
    void get().refresh();
  },

  refresh: async () => {
    const hits = await invoke<[Hit, Item][]>("search", {
      query: get().query,
      limit: SEARCH_LIMIT,
      sort: get().sort,
      desc: get().desc,
      filter: get().filter,
    });
    const size = await invoke<number>("library_size");
    set({ hits, librarySize: size });
  },

  select: async (i) => {
    const entry = get().hits[i];
    if (!entry) return;
    set({ selected: i });
    try {
      const loaded = await invoke<Loaded>("load", {
        id: entry[1].id,
        normalise: get().normalise,
      });
      set({ current: loaded, region: [0, loaded.frames], view: [0, loaded.frames] });
      get().playRegion([0, loaded.frames]);
    } catch (e) {
      set({ message: `decode failed: ${e}` });
    }
  },

  // Similar-sound results are not necessarily in the current result list, so
  // this loads by id and only syncs the list selection when it happens to be
  // visible.
  selectById: async (id) => {
    const i = get().hits.findIndex(([, item]) => item.id === id);
    if (i >= 0) {
      await get().select(i);
      return;
    }
    try {
      const loaded = await invoke<Loaded>("load", { id, normalise: get().normalise });
      set({ current: loaded, region: [0, loaded.frames], view: [0, loaded.frames], selected: -1 });
      get().playRegion([0, loaded.frames]);
    } catch (e) {
      set({ message: `decode failed: ${e}` });
    }
  },

  setSimilarGate: (similarGate) => {
    set({ similarGate });
    saveSettings({ similarGate });
  },

  // The list row is the source of truth for tags; the loaded audio payload
  // deliberately carries none so tag edits do not require a re-decode.
  currentItem: () => {
    const { current, hits } = get();
    if (!current) return null;
    return hits.find(([, i]) => i.id === current.id)?.[1] ?? null;
  },

  beginRename: () => {
    if (get().currentItem()) set({ renaming: true });
  },

  cancelRename: () => set({ renaming: false }),

  commitRename: async (stem) => {
    const item = get().currentItem();
    if (!item) return;
    const previous = item.filename;
    try {
      const r = await invoke<{ filename: string; suffixed: boolean }>("rename_file", {
        id: item.id,
        stem,
        allowSuffix: true,
      });
      set({
        renaming: false,
        message: r.suffixed
          ? `name taken, saved as ${r.filename}`
          : `${previous} → ${r.filename}   (Ctrl+Z to undo)`,
      });
      await get().refresh();
    } catch (e) {
      set({ message: `rename failed: ${e}` });
    }
  },

  undoRename: async () => {
    try {
      const name = await invoke<string | null>("undo_rename");
      set({ message: name ? `renamed back to ${name}` : "nothing to undo" });
      await get().refresh();
    } catch (e) {
      set({ message: `undo failed: ${e}` });
    }
  },

  toggleFavorite: async (id) => {
    await invoke("toggle_favorite", { id });
    await get().refresh();
  },

  addTag: async (id, tag) => {
    await invoke("tag_file", { id, tag });
    await get().loadTags();
    await get().refresh();
  },

  removeTag: async (id, tag) => {
    await invoke("untag_file", { id, tag });
    await get().loadTags();
    await get().refresh();
  },

  setBinding: (id, combos) => {
    const keymap = { ...get().keymap, [id]: combos };
    set({ keymap });
    saveSettings({ keymap });
  },

  resetBinding: (id) => {
    // Removing the override rather than writing the current default, so a
    // default changed in a later release still carries over.
    const keymap = { ...get().keymap };
    delete keymap[id];
    set({ keymap });
    saveSettings({ keymap });
  },

  resetBindings: () => {
    set({ keymap: {} });
    saveSettings({ keymap: {} });
  },

  setShowShortcuts: (showShortcuts) => set({ showShortcuts }),

  loadTags: async () => {
    const tags = await invoke<[string, number][]>("tags");
    // Filtering by a tag that no longer exists would show an empty list with
    // no obvious way back.
    const { filter } = get();
    const stale = filter.tag && !tags.some(([name]) => name === filter.tag);
    set(stale ? { tags, filter: { ...filter, tag: null } } : { tags });
  },

  setFilter: async (filter) => {
    set({ filter });
    await get().refresh();
  },

  move: async (delta) => {
    const { selected, hits } = get();
    const next = Math.max(0, Math.min(hits.length - 1, selected + delta));
    if (next !== selected) await get().select(next);
  },

  setRegion: (region) => set({ region }),

  setView: (view) => set({ view }),

  zoom: (factor) => {
    const { current, view, pos, playing } = get();
    if (!current || !view) return;
    const span = view[1] - view[0];
    // Zoom about the playhead when it is on screen, otherwise the view centre.
    const anchor = playing && pos >= view[0] && pos <= view[1] ? pos : view[0] + span / 2;
    const next = Math.min(current.frames, Math.max(256, Math.round(span * factor)));
    let start = Math.round(anchor - ((anchor - view[0]) / span) * next);
    start = Math.max(0, Math.min(current.frames - next, start));
    set({ view: [start, start + next] });
  },

  zoomToFit: () => {
    const { current } = get();
    if (current) set({ view: [0, current.frames] });
  },

  zoomToRegion: () => {
    const { region, current } = get();
    if (!region || !current || region[1] - region[0] < 256) return;
    set({ view: [region[0], region[1]] });
  },

  markIn: () => {
    const { region, pos, current } = get();
    if (!current || !region) return;
    const start = Math.min(pos, region[1] - 1);
    set({ region: [Math.max(0, start), region[1]] });
  },

  markOut: () => {
    const { region, pos, current } = get();
    if (!current || !region) return;
    const end = Math.max(pos, region[0] + 1);
    set({ region: [region[0], Math.min(current.frames, end)] });
  },

  playRegion: (r) => {
    const region = r ?? get().region;
    if (!region) return;
    set({ region });
    void invoke("play", { start: region[0], end: region[1], looping: get().looping });
  },

  toggle: () => void invoke("toggle"),
  stop: () => void invoke("stop"),

  setLooping: (looping) => {
    set({ looping });
    saveSettings({ looping });
    void invoke("set_looping", { looping });
  },

  setNormalise: async (normalise) => {
    set({ normalise });
    saveSettings({ normalise });
    // Gain is applied at load time, so re-load to hear the change.
    const { selected } = get();
    if (selected >= 0) await get().select(selected);
  },

  setVolume: (volume) => {
    set({ volume });
    saveSettings({ volume });
    void invoke("set_volume", { volume });
  },

  setSort: async (sort) => {
    set({ sort });
    saveSettings({ sort });
    await get().refresh();
  },

  // Explorer behaviour: a new column starts ascending, clicking the active
  // column flips direction.
  sortBy: async (key) => {
    const { sort, desc } = get();
    const next = sort === key ? { sort, desc: !desc } : { sort: key, desc: false };
    set(next);
    saveSettings(next);
    await get().refresh();
  },

  loadRoots: async () => {
    const rows = await invoke<[number, string, string][]>("roots");
    set({ roots: rows.map(([id, path, label]) => ({ id, path, label })) });
  },

  removeRoot: async (id) => {
    await invoke("remove_root", { id });
    set({ selected: -1, current: null, region: null });
    await get().loadRoots();
    await get().refresh();
  },

  rescanRoot: async (id) => {
    set({ scanning: { done: 0, total: 0 }, message: "rescanning…" });
    try {
      const n = await invoke<number>("rescan_root", { id });
      clearSparks();
      set({ message: `${n} sounds indexed` });
      await get().refresh();
    } catch (e) {
      set({ message: `rescan failed: ${e}` });
    } finally {
      set({ scanning: null });
    }
  },

  setLayout: (layout) => set({ layout }),

  addFolder: async () => {
    const dir = await invoke<string | null>("pick_folder");
    if (!dir) return;
    set({ scanning: { done: 0, total: 0 }, message: `scanning ${dir}` });
    try {
      const n = await invoke<number>("add_root", { path: dir });
      clearSparks();
      set({ message: `indexed ${n} files` });
      await get().loadRoots();
      await get().refresh();
      // A pack shipped with the sounds is the whole point of packs.
      const found = await invoke<string | null>("pack_in_root", { path: dir });
      if (found && confirm(`This folder ships tags in ${found.split(/[/\\]/).pop()}.\n\nImport them?`)) {
        await get().importPack(found);
      }
    } catch (e) {
      set({ message: `scan failed: ${e}` });
    } finally {
      set({ scanning: null });
    }
  },

  loadProfiles: async () => {
    set({ registry: await invoke<Registry>("profiles_list") });
  },

  switchProfile: async (id) => {
    const n = await invoke<number>("profile_switch", { id });
    await get().loadProfiles();
    await get().loadRoots();
    await get().loadTags();
    clearSparks();
    set({ query: "", selected: -1, current: null, message: `${n} sounds` });
    await get().refresh();
  },

  createProfile: async (name) => {
    try {
      const reg = await invoke<Registry>("profile_create", { name });
      set({ registry: reg });
      const made = reg.profiles.find((p) => p.name === name.trim());
      if (made) await get().switchProfile(made.id);
    } catch (e) {
      set({ message: `${e}` });
    }
  },

  renameProfile: async (id, name) => {
    try {
      set({ registry: await invoke<Registry>("profile_rename", { id, name }) });
    } catch (e) {
      set({ message: `${e}` });
    }
  },

  deleteProfile: async (id) => {
    try {
      set({ registry: await invoke<Registry>("profile_delete", { id }) });
      await get().loadRoots();
      await get().loadTags();
      clearSparks();
      set({ selected: -1, current: null });
      await get().refresh();
    } catch (e) {
      set({ message: `${e}` });
    }
  },

  exportPack: async () => {
    const path = await invoke<string | null>("save_pack_dialog");
    if (!path) return;
    try {
      const n = await invoke<number>("pack_export", { path });
      set({ message: `exported ${n} tagged sounds` });
    } catch (e) {
      set({ message: `export failed: ${e}` });
    }
  },

  importPack: async (path) => {
    const file = path ?? (await invoke<string | null>("open_pack_dialog"));
    if (!file) return;
    try {
      const p = await invoke<PackPreview>("pack_preview", { path: file });
      const lines = [
        `${p.exact} matched exactly`,
        p.fuzzy ? `${p.fuzzy} matched by name and size only` : "",
        p.missing ? `${p.missing} not in this library` : "",
      ].filter(Boolean);
      const useFuzzy =
        p.fuzzy > 0 &&
        confirm(
          `${lines.join("\n")}\n\nInclude the ${p.fuzzy} uncertain matches?\n` +
            `These share a name and size but not contents, so they may be re-encodes.`,
        );
      if (!confirm(`Import tags for ${p.exact + (useFuzzy ? p.fuzzy : 0)} sounds?`)) return;
      const applied = await invoke<PackPreview>("pack_import", {
        path: file,
        overwrite: false,
        includeFuzzy: useFuzzy,
      });
      set({ message: `imported ${applied.exact + applied.fuzzy} sounds` });
      await get().loadTags();
      await get().refresh();
    } catch (e) {
      set({ message: `import failed: ${e}` });
    }
  },

  tick: async () => {
    const s = await invoke<{ pos: number; playing: boolean }>("status");
    if (s.playing !== get().playing || s.pos !== get().pos) {
      set({ pos: s.pos, playing: s.playing });
    }
  },
}));
