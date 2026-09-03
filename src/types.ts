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
