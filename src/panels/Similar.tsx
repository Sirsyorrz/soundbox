import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useStore } from "../state/store";
import type { Item } from "../types";

interface SimilarDto {
  item: Item;
  score: number;
}

export function Similar() {
  const current = useStore((s) => s.current);
  const selectById = useStore((s) => s.selectById);
  const gate = useStore((s) => s.similarGate);
  const setGate = useStore((s) => s.setSimilarGate);
  const filter = useStore((s) => s.filter);

  const [rows, setRows] = useState<SimilarDto[]>([]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!current) {
      setRows([]);
      return;
    }
    let stale = false;
    setBusy(true);
    invoke<SimilarDto[]>("similar", { id: current.id, limit: 40, gate, filter })
      .then((r) => {
        if (!stale) setRows(r);
      })
      .finally(() => {
        if (!stale) setBusy(false);
      });
    return () => {
      stale = true;
    };
  }, [current, gate, filter]);

  return (
    <div className="similar">
      <div className="sidebar-h row-h">
        <span>Similar sounds</span>
        <span className="spacer" />
        <label title="Only compare sounds of comparable length">
          <input type="checkbox" checked={gate} onChange={(e) => setGate(e.target.checked)} />
          similar length
        </label>
      </div>
      {!current && <div className="dim pad">select a sound</div>}
      {current && rows.length === 0 && !busy && <div className="dim pad">no close matches</div>}
      <div className="simlist">
        {rows.map((r) => (
          <div
            className="simrow"
            key={r.item.id}
            onClick={() => void selectById(r.item.id)}
            title={`${r.item.folder}/${r.item.filename}`}
          >
            <div className="bar" style={{ width: `${Math.max(0, r.score) * 100}%` }} />
            <span className="simname">{r.item.filename}</span>
            <span className="simscore">{r.score.toFixed(3)}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
