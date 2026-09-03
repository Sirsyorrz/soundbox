import { useEffect, useRef, useState } from "react";
import { useStore } from "../state/store";

export function TagEditor() {
  const item = useStore((s) => s.currentItem());
  const addTag = useStore((s) => s.addTag);
  const removeTag = useStore((s) => s.removeTag);
  const toggleFavorite = useStore((s) => s.toggleFavorite);
  const [draft, setDraft] = useState("");

  if (!item) return null;

  return (
    <div className="tagbar">
      <button
        className={"star" + (item.favorite ? " on" : "")}
        title="Favourite  ( F )"
        onClick={() => void toggleFavorite(item.id)}
      >
        {item.favorite ? "★" : "☆"}
      </button>
      {item.tags.map((t) => (
        <span className="chip" key={t}>
          {t}
          <button title={`Remove tag "${t}"`} onClick={() => void removeTag(item.id, t)}>
            ×
          </button>
        </span>
      ))}
      <input
        id="tag-input"
        className="taginput"
        value={draft}
        placeholder="+ tag"
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && draft.trim()) {
            void addTag(item.id, draft.trim());
            setDraft("");
          }
          if (e.key === "Escape") (e.target as HTMLInputElement).blur();
        }}
      />
    </div>
  );
}

export function RenameBox() {
  const item = useStore((s) => s.currentItem());
  const renaming = useStore((s) => s.renaming);
  const cancel = useStore((s) => s.cancelRename);
  const commit = useStore((s) => s.commitRename);
  const [draft, setDraft] = useState("");
  const ref = useRef<HTMLInputElement>(null);

  const stem = item ? item.filename.replace(/\.[^.]+$/, "") : "";

  useEffect(() => {
    if (renaming) {
      setDraft(stem);
      // Select the stem so typing replaces it, as file managers do.
      requestAnimationFrame(() => {
        ref.current?.focus();
        ref.current?.select();
      });
    }
  }, [renaming, stem]);

  if (!renaming || !item) return null;
  const ext = item.filename.slice(stem.length);

  return (
    <div className="renamebox">
      <input
        ref={ref}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter" && draft.trim()) void commit(draft.trim());
          if (e.key === "Escape") cancel();
        }}
      />
      <span className="ext">{ext}</span>
      <button onClick={() => draft.trim() && void commit(draft.trim())}>Rename</button>
      <button onClick={cancel}>Cancel</button>
    </div>
  );
}

export function TagRail() {
  const tags = useStore((s) => s.tags);
  const filter = useStore((s) => s.filter);
  const setFilter = useStore((s) => s.setFilter);

  return (
    <>
      <div className="sidebar-h">Filters</div>
      <div
        className={"tagrow" + (filter.favoritesOnly ? " on" : "")}
        onClick={() => void setFilter({ ...filter, favoritesOnly: !filter.favoritesOnly })}
      >
        <span>★ Favourites</span>
      </div>
      {tags.length === 0 && <div className="dim pad">no tags yet</div>}
      {tags.map(([name, count]) => (
        <div
          key={name}
          className={"tagrow" + (filter.tag === name ? " on" : "")}
          onClick={() => void setFilter({ ...filter, tag: filter.tag === name ? null : name })}
        >
          <span className="tagname">{name}</span>
          <span className="tagcount">{count}</span>
        </div>
      ))}
    </>
  );
}
