export interface Item {
  id: number;
  filename: string;
  folder: string;
  duration_ms: number;
  channels: number;
  sample_rate: number;
  ext: string;
}

export interface Hit {
  id: number;
  score: number;
  indices: number[];
  via_folder: boolean;
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
