/**
 * Every keyboard-triggered behaviour is declared here once. The shortcuts panel
 * is generated from this list, so a new action becomes remappable with no extra
 * UI work.
 *
 * Bindings use `KeyboardEvent.code` ("KeyF", "Space") rather than characters, so
 * they survive non-QWERTY layouts.
 */
export interface ActionDef {
  id: string;
  label: string;
  category: string;
  /** Default binding, e.g. "Space", "Ctrl+KeyZ", "BracketLeft". */
  default: string;
  /** Bindings only apply while this context is active. */
  context?: "global" | "search";
}

export const ACTIONS: ActionDef[] = [
  { id: "search.focus", label: "Focus search", category: "Search", default: "Slash" },
  { id: "search.clear", label: "Clear search", category: "Search", default: "Escape", context: "search" },

  { id: "transport.playPause", label: "Play / pause", category: "Transport", default: "Space" },
  { id: "transport.playRegion", label: "Play region", category: "Transport", default: "Enter" },
  { id: "transport.loop", label: "Toggle loop", category: "Transport", default: "KeyL" },

  { id: "nav.up", label: "Previous sound", category: "Navigation", default: "ArrowUp" },
  { id: "nav.down", label: "Next sound", category: "Navigation", default: "ArrowDown" },

  { id: "region.in", label: "Set region in", category: "Region", default: "BracketLeft" },
  { id: "region.out", label: "Set region out", category: "Region", default: "BracketRight" },

  { id: "zoom.in", label: "Zoom in", category: "Zoom", default: "Equal" },
  { id: "zoom.out", label: "Zoom out", category: "Zoom", default: "Minus" },
  { id: "zoom.fit", label: "Zoom to fit", category: "Zoom", default: "Digit0" },
  { id: "zoom.region", label: "Zoom to region", category: "Zoom", default: "KeyZ" },

  { id: "organise.favorite", label: "Toggle favourite", category: "Organise", default: "KeyF" },
  { id: "organise.tag", label: "Add tag", category: "Organise", default: "KeyT" },
  { id: "organise.rename", label: "Rename file", category: "Organise", default: "F2" },
  { id: "organise.undo", label: "Undo rename", category: "Organise", default: "Ctrl+KeyZ" },

  { id: "app.shortcuts", label: "Keyboard shortcuts", category: "App", default: "Ctrl+Slash" },
];

export type Keymap = Record<string, string[]>;

/** The combination string for an event, matching the format used above. */
export function comboOf(e: KeyboardEvent | React.KeyboardEvent): string {
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Meta");
  parts.push(e.code);
  return parts.join("+");
}

/** Modifier-only presses are not bindings; they are half of one. */
export function isModifierOnly(code: string): boolean {
  return /^(Control|Alt|Shift|Meta)(Left|Right)$/.test(code);
}

export function bindingsFor(map: Keymap, id: string): string[] {
  const custom = map[id];
  if (custom) return custom;
  const def = ACTIONS.find((a) => a.id === id);
  return def ? [def.default] : [];
}

/** combo -> action id, for the whole keymap. */
export function resolveTable(map: Keymap): Map<string, string> {
  const table = new Map<string, string>();
  for (const a of ACTIONS) {
    for (const combo of bindingsFor(map, a.id)) table.set(combo, a.id);
  }
  return table;
}

/** Actions already using a combo, so the UI can warn before overwriting. */
export function conflictsWith(map: Keymap, combo: string, exceptId: string): string[] {
  return ACTIONS.filter((a) => a.id !== exceptId && bindingsFor(map, a.id).includes(combo)).map(
    (a) => a.id,
  );
}

/** Pretty form for display: "Ctrl+KeyZ" -> "Ctrl + Z". */
export function prettyCombo(combo: string): string {
  return combo
    .split("+")
    .map((p) =>
      p
        .replace(/^Key/, "")
        .replace(/^Digit/, "")
        .replace("BracketLeft", "[")
        .replace("BracketRight", "]")
        .replace("Equal", "+")
        .replace("Minus", "-")
        .replace("Slash", "/")
        .replace("ArrowUp", "↑")
        .replace("ArrowDown", "↓")
        .replace("ArrowLeft", "←")
        .replace("ArrowRight", "→"),
    )
    .join(" + ");
}
