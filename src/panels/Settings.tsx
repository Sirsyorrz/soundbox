import { useStore } from "../state/store";

/** One place for everything that is not part of browsing the library. */
export function Settings() {
  const open = useStore((s) => s.showSettings);
  const close = () => useStore.getState().setShowSettings(false);
  const pruneCache = useStore((s) => s.pruneCache);
  const setShowShortcuts = useStore((s) => s.setShowShortcuts);

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
