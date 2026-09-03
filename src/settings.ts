import type { Keymap } from "./actions";
import type { Sort } from "./types";

export interface Settings {
  volume: number;
  normalise: boolean;
  similarGate: boolean;
  looping: boolean;
  sort: Sort;
  desc: boolean;
  keymap: Keymap;
}

export const DEFAULTS: Settings = {
  volume: 1,
  normalise: true,
  similarGate: true,
  looping: false,
  sort: "relevance",
  desc: false,
  keymap: {},
};

const KEY = "soundbox.settings";

export function loadSettings(): Settings {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return { ...DEFAULTS };
    // Merge over defaults so a setting added in a later version is not missing.
    return { ...DEFAULTS, ...JSON.parse(raw) };
  } catch {
    return { ...DEFAULTS };
  }
}

let pending: number | undefined;

/** Debounced: dragging the volume slider would otherwise write on every frame. */
export function saveSettings(s: Partial<Settings>) {
  const merged = { ...loadSettings(), ...s };
  clearTimeout(pending);
  pending = setTimeout(() => {
    try {
      localStorage.setItem(KEY, JSON.stringify(merged));
    } catch {
      // Storage being unavailable must not break playback.
    }
  }, 150) as unknown as number;
}
