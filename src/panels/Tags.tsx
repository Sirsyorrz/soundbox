import { useState } from "react";
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
