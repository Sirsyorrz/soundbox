import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { Hit, Item, Loaded } from "../types";
import type { LayoutName } from "../layout";

const SEARCH_LIMIT = 500;

interface State {
  query: string;
  hits: [Hit, Item][];
  selected: number;
  current: Loaded | null;
  region: [number, number] | null;
  playing: boolean;
  pos: number;
  looping: boolean;
  normalise: boolean;
  librarySize: number;
  scanning: { done: number; total: number } | null;
  device: string;
  layout: LayoutName;
  message: string;

  setQuery: (q: string) => void;
  refresh: () => Promise<void>;
  select: (i: number) => Promise<void>;
  move: (delta: number) => Promise<void>;
  setRegion: (r: [number, number] | null) => void;
  playRegion: (r?: [number, number]) => void;
  toggle: () => void;
  stop: () => void;
  setLooping: (v: boolean) => void;
  setNormalise: (v: boolean) => Promise<void>;
  setLayout: (l: LayoutName) => void;
  addFolder: () => Promise<void>;
  tick: () => Promise<void>;
  say: (m: string) => void;
}

export const useStore = create<State>((set, get) => ({
  query: "",
  hits: [],
  selected: -1,
  current: null,
  region: null,
  playing: false,
  pos: 0,
  looping: false,
  normalise: true,
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
      set({ current: loaded, region: [0, loaded.frames] });
      get().playRegion([0, loaded.frames]);
    } catch (e) {
      set({ message: `decode failed: ${e}` });
    }
  },

  move: async (delta) => {
    const { selected, hits } = get();
    const next = Math.max(0, Math.min(hits.length - 1, selected + delta));
    if (next !== selected) await get().select(next);
  },

  setRegion: (region) => set({ region }),

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
    void invoke("set_looping", { looping });
  },

  setNormalise: async (normalise) => {
    set({ normalise });
    // Gain is applied at load time, so re-load to hear the change.
    const { selected } = get();
    if (selected >= 0) await get().select(selected);
  },

  setLayout: (layout) => set({ layout }),

  addFolder: async () => {
    const dir = await invoke<string | null>("pick_folder");
    if (!dir) return;
    set({ scanning: { done: 0, total: 0 }, message: `scanning ${dir}` });
    try {
      const n = await invoke<number>("add_root", { path: dir });
      set({ message: `indexed ${n} files` });
      await get().refresh();
    } catch (e) {
      set({ message: `scan failed: ${e}` });
    } finally {
      set({ scanning: null });
    }
  },

  tick: async () => {
    const s = await invoke<{ pos: number; playing: boolean }>("status");
    if (s.playing !== get().playing || s.pos !== get().pos) {
      set({ pos: s.pos, playing: s.playing });
    }
  },
}));
