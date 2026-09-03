import { useStore } from "../state/store";

/** One place for everything that is not part of browsing the library. */
export function Settings() {
  const open = useStore((s) => s.showSettings);
  const close = () => useStore.getState().setShowSettings(false);
  const pruneCache = useStore((s) => s.pruneCache);
  const setShowShortcuts = useStore((s) => s.setShowShortcuts);
  const clearTags = useStore((s) => s.clearTags);
  const exportPack = useStore((s) => s.exportPack);
  // Filtered in the render body, not the selector: a selector returning a new
  // array every call makes useSyncExternalStore throw and blanks the app.
  const tags = useStore((s) => s.tags);
  const userTags = tags.filter(([, , isUser]) => isUser);

  if (!open) return null;

  return (
    <div className="modal-back" onClick={close}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-h">
          <b>Settings</b>
          <span className="spacer" />
          <button onClick={close}>Close</button>
        </div>

        <div className="modal-body">
          <div className="sidebar-h">Keyboard</div>
          <div className="setrow">
            <div className="setlabel">
              Shortcuts
              <div className="dim">Rebind any key. Ctrl + / opens this directly.</div>
            </div>
            <button
              onClick={() => {
                close();
                setShowShortcuts(true);
              }}
            >
              Edit…
            </button>
          </div>

          <div className="sidebar-h">Storage</div>
          <div className="setrow">
            <div className="setlabel">
              Clean waveform cache
              <div className="dim">
                Deletes cached waveforms no profile refers to any more. Nothing is
                recomputed until you play the sound again.
              </div>
            </div>
            <button onClick={() => void pruneCache()}>Clean</button>
          </div>

          <div className="sidebar-h">Tags</div>
          <div className="setrow danger">
            <div className="setlabel">
              Clear all tags
              <div className="dim">
                Removes every tag you have added, from every sound in this profile.
                Favourites and folder names are not affected. This cannot be undone.
              </div>
            </div>
            <button
              className="danger"
              disabled={userTags.length === 0}
              onClick={async () => {
                const names = userTags.map(([n]) => n);
                const preview = names.slice(0, 8).join(", ");
                const more = names.length > 8 ? `, and ${names.length - 8} more` : "";
                if (
                  !confirm(
                    `Clear ${names.length} tags from this profile?\n\n${preview}${more}\n\n` +
                      `Favourites and folder names are kept. This cannot be undone.`,
                  )
                )
                  return;
                // Offer the one route back before doing something irreversible.
                if (
                  confirm("Save a copy of these tags to a file first?\n\nRecommended.")
                ) {
                  await exportPack();
                }
                if (!confirm("Last chance. Clear all tags now?")) return;
                await clearTags();
                close();
              }}
            >
              Clear…
            </button>
          </div>

          <div className="sidebar-h">Updates</div>
          <div className="setrow">
            <div className="setlabel">
              Check for updates
              <div className="dim">Updates are checked automatically on launch.</div>
            </div>
            <button
              onClick={() => {
                close();
                useStore.setState((v) => ({ updateNonce: v.updateNonce + 1 }));
              }}
            >
              Check
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
