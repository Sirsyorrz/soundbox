import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type {
  Filter,
  FolderNode,
  Hit,
  ImportFormat,
  Item,
  Job,
  Loaded,
  PackPreview,
  Registry,
  Root,
  Sort,
  ToolStatus,
} from "../types";
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
  /** Visible frame range of the detail waveform. */
  view: [number, number] | null;
  playing: boolean;
  pos: number;
  normalise: boolean;
  volume: number;
  sort: Sort;
  desc: boolean;
  rowH: number;
  similarGate: boolean;
  roots: Root[];
  folders: FolderNode[];
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
  setView: (v: [number, number]) => void;
  zoom: (factor: number) => void;
  zoomToFit: () => void;
  playFrom: (frame: number) => void;
  toggle: () => void;
  stop: () => void;
  setNormalise: (v: boolean) => Promise<void>;
  setVolume: (v: number) => void;
  setSort: (s: Sort) => Promise<void>;
  sortBy: (s: Sort) => Promise<void>;
  loadRoots: () => Promise<void>;
  loadFolders: () => Promise<void>;
  removeRoot: (id: number) => Promise<void>;
  rescanRoot: (id: number) => Promise<void>;
  setLayout: (l: LayoutName) => void;
  addFolder: (dir?: string) => Promise<boolean>;
  registry: Registry | null;
  updateNonce: number;
  keymap: Keymap;
  setBinding: (id: string, combos: string[]) => void;
  resetBinding: (id: string) => void;
  resetBindings: () => void;
  failed: [string, string][];
  loadFailed: () => Promise<void>;
  showFailed: boolean;
  setShowFailed: (v: boolean) => void;
  cancelScan: () => void;
  pruneCache: () => Promise<void>;
  clearTags: () => Promise<void>;
  showSettings: boolean;
  setShowSettings: (v: boolean) => void;
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

  jobs: Job[];
  tools: ToolStatus | null;
  /** Set while yt-dlp, Deno or ffmpeg are being fetched. */
  toolProgress: { stage: string; done: number; total: number | null } | null;
  showImport: boolean;
  setShowImport: (v: boolean) => void;
  importFormat: ImportFormat;
  setImportFormat: (f: ImportFormat) => void;
  importPlaylist: boolean;
  setImportPlaylist: (v: boolean) => void;
  importDest: Record<string, string>;
  /** The active profile's import folder, falling back to its first root. */
  importDestPath: () => string | null;
  setImportDest: (path: string) => void;
  browseImportDest: () => Promise<void>;
  loadTools: () => Promise<ToolStatus>;
  ensureTools: (ffmpeg: boolean) => Promise<boolean>;
  startImport: (
    url: string,
    opts?: { format?: ImportFormat; playlist?: boolean; clip?: [string, string] | null },
  ) => Promise<boolean>;
  cancelImport: (id: number) => void;
  dismissJob: (id: number) => void;
  clearFinishedJobs: () => void;
  onJob: (job: Job) => Promise<void>;
  updateYtdlp: () => Promise<void>;
  configureTools: (channel: "stable" | "nightly", useSystem: boolean) => Promise<void>;
  removeTools: () => Promise<void>;
}

const FINISHED = new Set(["done", "failed", "cancelled"]);

export const isFinished = (j: Job) => FINISHED.has(j.state);

const SAVED = loadSettings();

export const useStore = create<State>((set, get) => ({
  query: "",
  hits: [],
  selected: -1,
  current: null,
  view: null,
  playing: false,
  pos: 0,
  normalise: SAVED.normalise,
  volume: SAVED.volume,
  registry: null,
  updateNonce: 0,
  keymap: SAVED.keymap,
  failed: [],
  showFailed: false,
  showSettings: false,
  showShortcuts: false,
  sort: SAVED.sort,
  desc: SAVED.desc,
  rowH: 26,
  similarGate: true,
  roots: [],
  folders: [],
  tags: [],
  filter: { favoritesOnly: false, tag: null, root: null, folder: null },
  renaming: false,
  librarySize: 0,
  scanning: null,
  device: "",
  layout: "default",
  message: "",
  jobs: [],
  tools: null,
  toolProgress: null,
  showImport: false,
  importFormat: SAVED.importFormat,
  importPlaylist: SAVED.importPlaylist,
  importDest: SAVED.importDest,

  say: (message) => set({ message }),

  setShowImport: (showImport) => set({ showImport }),

  setImportFormat: (importFormat) => {
    set({ importFormat });
    saveSettings({ importFormat });
  },

  setImportPlaylist: (importPlaylist) => {
    set({ importPlaylist });
    saveSettings({ importPlaylist });
  },

  importDestPath: () => {
    const { registry, importDest, roots } = get();
    const saved = registry ? importDest[registry.active] : undefined;
    return saved ?? roots[0]?.path ?? null;
  },

  setImportDest: (path) => {
    const active = get().registry?.active;
    if (!active) return;
    const importDest = { ...get().importDest, [active]: path };
    set({ importDest });
    saveSettings({ importDest });
  },

  browseImportDest: async () => {
    const dir = await invoke<string | null>("pick_folder");
    if (!dir) return;
    const root = await invoke<number | null>("root_for", { path: dir });
    if (root == null) {
      // Imports land on disk either way, but outside the library they would
      // never show up, which looks like the import failed.
      const add = confirm(
        `${dir}\n\nis not in this profile's library, so downloads saved there would not show up.\n\n` +
          `Add it as a library folder?`,
      );
      if (!add || !(await get().addFolder(dir))) return;
    }
    get().setImportDest(dir);
  },

  loadTools: async () => {
    const tools = await invoke<ToolStatus>("tools_status");
    set({ tools });
    return tools;
  },

  ensureTools: async (ffmpeg) => {
    if (get().toolProgress) {
      set({ message: "setup is still running" });
      return false;
    }
    const t = await get().loadTools();
    if (!t.ytdlp.path && t.use_system_ytdlp) {
      set({ message: 'no yt-dlp on PATH. Install it or turn off "use system yt-dlp" in Settings' });
      return false;
    }
    if (!t.ytdlp.path && !t.supported) {
      set({ message: "no yt-dlp build for this platform; install it and enable system yt-dlp" });
      return false;
    }
    const missing: string[] = [];
    if (!t.ytdlp.path) missing.push("yt-dlp, the downloader (about 40 MB)");
    if (!t.deno.path) missing.push("Deno, which YouTube needs (about 40 MB)");
    if (ffmpeg && !t.ffmpeg.path)
      missing.push("ffmpeg, for converting and clipping (about 160 to 200 MB)");
    if (missing.length === 0) return true;
    if (
      !confirm(
        `Downloading needs:\n\n${missing.map((m) => `  ${m}`).join("\n")}\n\n` +
          `These are downloaded once into SoundBox's data folder. yt-dlp then keeps itself up to date.`,
      )
    )
      return false;
    set({ toolProgress: { stage: "starting", done: 0, total: null } });
    try {
      set({ tools: await invoke<ToolStatus>("tools_install", { ffmpeg }) });
      return true;
    } catch (e) {
      set({ message: `setup failed: ${e}` });
      return false;
    } finally {
      set({ toolProgress: null });
    }
  },

  startImport: async (url, opts = {}) => {
    const dest = get().importDestPath();
    if (!dest) {
      set({ message: "add a library folder first, so downloads have somewhere to go" });
      return false;
    }
    const format = opts.format ?? get().importFormat;
    const playlist = opts.playlist ?? get().importPlaylist;
    const clip = opts.clip ?? null;
    if (!(await get().ensureTools(format !== "original" || clip != null))) return false;
    try {
      const req = { url: url.trim(), dest, format, playlist, clip };
      const id = await invoke<number>("download_start", { req });
      const job: Job = {
        id,
        state: "starting",
        url: req.url,
        dest,
        title: null,
        done: null,
        total: null,
        index: null,
        count: null,
        error: null,
        files: 0,
        outside: 0,
      };
      // The backend's first event can beat this, so never overwrite one.
      set((s) => (s.jobs.some((j) => j.id === id) ? {} : { jobs: [...s.jobs, job] }));
      return true;
    } catch (e) {
      set({ message: `download failed: ${e}` });
      return false;
    }
  },

  cancelImport: (id) => void invoke("download_cancel", { id }),

  dismissJob: (id) => set((s) => ({ jobs: s.jobs.filter((j) => j.id !== id) })),

  clearFinishedJobs: () => set((s) => ({ jobs: s.jobs.filter((j) => !isFinished(j)) })),

  onJob: async (job) => {
    if (job.error?.includes("Requested format is not available")) {
      job = {
        ...job,
        error: "no format SoundBox can play directly. Pick WAV, FLAC or MP3 in download options.",
      };
    }
    set((s) => {
      const i = s.jobs.findIndex((j) => j.id === job.id);
      if (i < 0) return { jobs: [...s.jobs, job] };
      const jobs = s.jobs.slice();
      jobs[i] = job;
      return { jobs };
    });
    if (!isFinished(job)) return;

    const name = job.title ?? job.url;
    const parts: string[] = [];
    if (job.state === "failed") parts.push(`download failed: ${job.error ?? name}`);
    else if (job.state === "cancelled") parts.push(`download cancelled: ${name}`);
    else parts.push(`downloaded ${job.files} sound${job.files === 1 ? "" : "s"}: ${name}`);
    if (job.outside) parts.push(`${job.outside} saved outside the library`);
    if (job.state === "done" && job.error) parts.push(`some failed: ${job.error}`);
    set({ message: parts.join(", ") });
    if (job.files) {
      await get().loadFolders();
      await get().loadFailed();
      await get().refresh();
    }
  },

  updateYtdlp: async () => {
    set({ message: "updating yt-dlp…" });
    try {
      set({ message: `yt-dlp: ${await invoke<string>("tools_update")}` });
    } catch (e) {
      set({ message: `yt-dlp update failed: ${e}` });
    }
    await get().loadTools();
  },

  configureTools: async (channel, useSystem) => {
    await invoke("tools_configure", { channel, useSystemYtdlp: useSystem });
    await get().loadTools();
  },

  removeTools: async () => {
    try {
      await invoke("tools_remove");
      set({ message: "removed downloaded tools" });
    } catch (e) {
      set({ message: `could not remove tools: ${e}` });
    }
    await get().loadTools();
  },

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
      set({ current: loaded, view: [0, loaded.frames] });
      get().playFrom(0);
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
      set({ current: loaded, view: [0, loaded.frames], selected: -1 });
      get().playFrom(0);
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

  setShowSettings: (showSettings) => set({ showSettings }),

  setShowShortcuts: (showShortcuts) => set({ showShortcuts }),

  loadFailed: async () => {
    set({ failed: await invoke<[string, string][]>("failed_files") });
  },

  setShowFailed: (showFailed) => set({ showFailed }),

  cancelScan: () => {
    void invoke("cancel_scan");
    set({ message: "stopping scan…" });
  },

  clearTags: async () => {
    if (get().tags.length === 0) {
      set({ message: "no tags to clear" });
      return;
    }
    try {
      const [tags, sounds] = await invoke<[number, number]>("clear_tags");
      set({
        message: `cleared ${tags} tag${tags === 1 ? "" : "s"} from ${sounds} sound${sounds === 1 ? "" : "s"}`,
      });
      await get().loadTags();
      await get().refresh();
    } catch (e) {
      set({ message: `could not clear tags: ${e}` });
    }
  },

  pruneCache: async () => {
    const r = await invoke<{ removed: number; bytes: number }>("prune_cache");
    const mb = (r.bytes / 1048576).toFixed(1);
    set({
      message: r.removed
        ? `removed ${r.removed.toLocaleString()} unused waveforms (${mb} MB)`
        : "cache is already clean",
    });
  },

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

  playFrom: (frame) => {
    const { current } = get();
    if (!current) return;
    const start = Math.max(0, Math.min(current.frames, Math.round(frame)));
    void invoke("play", { start, end: current.frames });
  },

  toggle: () => void invoke("toggle"),
  stop: () => void invoke("stop"),

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
    const roots = rows.map(([id, path, label]) => ({ id, path, label }));
    // Filtering by a folder that is gone would show an empty list with no
    // obvious way back.
    const { filter } = get();
    const stale = filter.root != null && !roots.some((r) => r.id === filter.root);
    set(stale ? { roots, filter: { ...filter, root: null, folder: null } } : { roots });
    await get().loadFolders();
  },

  loadFolders: async () => {
    const rows = await invoke<[number, string, number][]>("folders");
    const folders = rows.map(([root, path, count]) => ({ root, path, count }));
    const { filter } = get();
    const stale =
      filter.folder != null &&
      !folders.some((f) => f.root === filter.root && f.path === filter.folder);
    set(stale ? { folders, filter: { ...filter, folder: null } } : { folders });
  },

  removeRoot: async (id) => {
    await invoke("remove_root", { id });
    set({ selected: -1, current: null });
    await get().loadRoots();
    await get().refresh();
  },

  rescanRoot: async (id) => {
    set({ scanning: { done: 0, total: 0 }, message: "rescanning…" });
    try {
      const n = await invoke<number>("rescan_root", { id });
      clearSparks();
      set({ message: `${n} sounds indexed` });
      await get().loadFolders();
      await get().loadFailed();
      await get().refresh();
    } catch (e) {
      set({ message: `rescan failed: ${e}` });
    } finally {
      set({ scanning: null });
    }
  },

  setLayout: (layout) => set({ layout }),

  addFolder: async (picked) => {
    const dir = picked ?? (await invoke<string | null>("pick_folder"));
    if (!dir) return false;
    set({ scanning: { done: 0, total: 0 }, message: `scanning ${dir}` });
    try {
      const n = await invoke<number>("add_root", { path: dir });
      clearSparks();
      set({ message: `indexed ${n} files` });
      await get().loadRoots();
      await get().loadFailed();
      await get().refresh();
      // A pack shipped with the sounds is the whole point of packs.
      const found = await invoke<string | null>("pack_in_root", { path: dir });
      if (found && confirm(`This folder ships tags in ${found.split(/[/\\]/).pop()}.\n\nImport them?`)) {
        await get().importPack(found);
      }
      return true;
    } catch (e) {
      set({ message: `scan failed: ${e}` });
      return false;
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
    set({
      query: "",
      selected: -1,
      current: null,
      filter: { ...get().filter, root: null, folder: null },
      message: `${n} sounds`,
    });
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
