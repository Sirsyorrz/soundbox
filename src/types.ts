export interface Item {
  id: number;
  filename: string;
  folder: string;
  duration_ms: number;
  channels: number;
  sample_rate: number;
  ext: string;
  mtime: number;
  added_at: number;
  last_played: number;
  favorite: boolean;
  tags: string[];
}

export interface Filter {
  favoritesOnly: boolean;
  tag: string | null;
  root: number | null;
  folder: string | null;
}

export interface Hit {
  id: number;
  score: number;
  indices: number[];
  via_folder: boolean;
}

export type Sort =
  | "relevance"
  | "name"
  | "folder"
  | "added"
  | "modified"
  | "recent"
  | "duration";

export interface Root {
  id: number;
  path: string;
  label: string;
}

/** A folder holding sounds, as reported by the backend. */
export interface FolderNode {
  root: number;
  path: string;
  count: number;
}

export interface Loaded {
  id: number;
  path: string;
  frames: number;
  duration_ms: number;
  sample_rate: number;
  channels: number;
  peaks: [number, number][][];
  lufs: number | null;
}

export interface Profile {
  id: string;
  name: string;
  color: string;
  last_opened: number;
}

export interface Registry {
  profiles: Profile[];
  active: string;
}

export interface PackPreview {
  total: number;
  exact: number;
  fuzzy: number;
  missing: number;
  sample_missing: string[];
}

export type ImportFormat = "original" | "wav" | "flac" | "mp3";

export interface ToolInfo {
  path: string | null;
  version: string | null;
  managed: boolean;
}

export interface ToolStatus {
  ytdlp: ToolInfo;
  deno: ToolInfo;
  ffmpeg: ToolInfo;
  channel: "stable" | "nightly";
  use_system_ytdlp: boolean;
  system_ytdlp: string | null;
  last_update: number;
  supported: boolean;
}

export type JobState =
  | "starting"
  | "updating"
  | "running"
  | "converting"
  | "done"
  | "failed"
  | "cancelled";

/** Mirrors the backend JobEvent; every event carries the full picture. */
export interface Job {
  id: number;
  state: JobState;
  url: string;
  dest: string;
  title: string | null;
  done: number | null;
  total: number | null;
  index: number | null;
  count: number | null;
  error: string | null;
  files: number;
  outside: number;
}
