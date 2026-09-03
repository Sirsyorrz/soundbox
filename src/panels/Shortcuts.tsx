import { useState } from "react";
import { useStore } from "../state/store";
import { ACTIONS, bindingsFor, comboOf, conflictsWith, isModifierOnly, prettyCombo } from "../actions";

export function Shortcuts() {
  const open = useStore((s) => s.showShortcuts);
  const close = () => useStore.getState().setShowShortcuts(false);
  const keymap = useStore((s) => s.keymap);
  const setBinding = useStore((s) => s.setBinding);
  const resetAll = useStore((s) => s.resetBindings);
  const resetOne = useStore((s) => s.resetBinding);
  const [capturing, setCapturing] = useState<string | null>(null);
  const [filter, setFilter] = useState("");

  if (!open) return null;

  const categories = [...new Set(ACTIONS.map((a) => a.category))];
  const q = filter.trim().toLowerCase();

  const capture = (id: string, e: React.KeyboardEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (e.code === "Escape") {
      setCapturing(null);
      return;
    }
    if (isModifierOnly(e.code)) return;

    const combo = comboOf(e);
    const clashes = conflictsWith(keymap, combo, id);
    if (clashes.length) {
      const names = clashes
        .map((c) => ACTIONS.find((a) => a.id === c)?.label ?? c)
        .join(", ");
      if (!confirm(`${prettyCombo(combo)} is already used by ${names}.\n\nReassign it?`)) return;
      // A combo may only mean one thing, so the loser is cleared.
      for (const c of clashes) {
        setBinding(
          c,
          bindingsFor(keymap, c).filter((b) => b !== combo),
        );
      }
    }
    setBinding(id, [combo]);
    setCapturing(null);
  };

  return (
    <div className="modal-back" onClick={close}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-h">
          <b>Keyboard shortcuts</b>
          <input
            placeholder="filter…"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            onKeyDown={(e) => e.stopPropagation()}
          />
          <span className="spacer" />
          <button onClick={resetAll} title="Restore every default binding">
            Reset all
          </button>
          <button onClick={close}>Close</button>
        </div>

        <div className="modal-body">
          {categories.map((cat) => {
            const rows = ACTIONS.filter(
              (a) =>
                a.category === cat &&
                (!q || a.label.toLowerCase().includes(q) || a.category.toLowerCase().includes(q)),
            );
            if (!rows.length) return null;
            return (
              <div key={cat}>
                <div className="sidebar-h">{cat}</div>
                {rows.map((a) => {
                  const combos = bindingsFor(keymap, a.id);
                  const changed = !!keymap[a.id];
                  return (
                    <div className="keyrow" key={a.id}>
                      <span className="keylabel">{a.label}</span>
                      <button
                        className={"keybind" + (capturing === a.id ? " capturing" : "")}
                        onKeyDown={(e) => capturing === a.id && capture(a.id, e)}
                        onClick={() => setCapturing(a.id)}
                        title="Click, then press the combination you want"
                      >
                        {capturing === a.id ? "press a key…" : combos.map(prettyCombo).join(", ")}
                      </button>
                      <button
                        className="x"
                        disabled={!changed}
                        title="Restore the default"
                        onClick={() => resetOne(a.id)}
                      >
                        ↺
                      </button>
                    </div>
                  );
                })}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
