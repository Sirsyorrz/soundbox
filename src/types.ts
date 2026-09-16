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
