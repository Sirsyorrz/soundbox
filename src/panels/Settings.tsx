import { useEffect } from "react";
import { isFinished, useStore } from "../state/store";
import type { ToolInfo } from "../types";

function where(t: ToolInfo) {
  if (!t.path) return "not installed";
  return `${t.version ?? "unknown version"}, ${t.managed ? "SoundBox's copy" : t.path}`;
}

function ImportSettings() {
  const tools = useStore((s) => s.tools);
  const busy = useStore((s) => s.jobs.some((j) => !isFinished(j)));
  const s = useStore.getState();

  useEffect(() => {
    void useStore.getState().loadTools();
  }, []);

  if (!tools) return <div className="pad dim">checking tools…</div>;
  const last = tools.last_update
    ? new Date(tools.last_update * 1000).toLocaleString()
    : "never";

  return (
    <>
      <div className="setrow">
        <div className="setlabel">
          yt-dlp
          <div className="dim">
            {where(tools.ytdlp)}
            {tools.ytdlp.managed && tools.ytdlp.path && (
              <>
                <br />
                Checks for updates once a day when you download. Last checked {last}.
              </>
            )}
          </div>
        </div>
        <button
          disabled={!tools.ytdlp.managed || !tools.ytdlp.path}
          title="Sites change often. If downloads start failing, update first."
          onClick={() => void s.updateYtdlp()}
        >
          Update now
        </button>
      </div>
      <div className="setrow">
        <div className="setlabel">
          Update channel
          <div className="dim">Nightly usually gets site fixes days before stable.</div>
        </div>
        <select
          value={tools.channel}
          disabled={tools.use_system_ytdlp}
          onChange={(e) =>
            void s.configureTools(e.target.value as "stable" | "nightly", tools.use_system_ytdlp)
          }
        >
          <option value="stable">stable</option>
          <option value="nightly">nightly</option>
        </select>
      </div>
      <div className="setrow">
        <div className="setlabel">
          Use system yt-dlp
          <div className="dim">
            {tools.system_ytdlp
              ? `Use ${tools.system_ytdlp} instead. Your package manager keeps it updated, not SoundBox.`
              : "No yt-dlp found on PATH."}
          </div>
        </div>
        <input
          type="checkbox"
          checked={tools.use_system_ytdlp}
          disabled={!tools.system_ytdlp && !tools.use_system_ytdlp}
          onChange={(e) => void s.configureTools(tools.channel, e.target.checked)}
        />
      </div>
      <div className="setrow">
        <div className="setlabel">
          Deno
          <div className="dim">{where(tools.deno)}. yt-dlp needs it for YouTube.</div>
        </div>
      </div>
      <div className="setrow">
        <div className="setlabel">
          ffmpeg
          <div className="dim">{where(tools.ffmpeg)}. Needed for converting and clipping.</div>
        </div>
        {!tools.ffmpeg.path && (
          <button onClick={() => void s.ensureTools(true)}>Get ffmpeg</button>
        )}
      </div>
      <div className="setrow">
        <div className="setlabel">
          Remove downloaded tools
          <div className="dim">
            Deletes SoundBox's copies of yt-dlp, Deno and ffmpeg. They are fetched again on the
            next download.
          </div>
        </div>
        <button
          disabled={busy || ![tools.ytdlp, tools.deno, tools.ffmpeg].some((t) => t.managed)}
          onClick={() => {
            if (confirm("Remove SoundBox's copies of yt-dlp, Deno and ffmpeg?")) void s.removeTools();
          }}
        >
          Remove
        </button>
      </div>
    </>
  );
}

/** One place for everything that is not part of browsing the library. */
export function Settings() {
  const open = useStore((s) => s.showSettings);
  const close = () => useStore.getState().setShowSettings(false);
  const pruneCache = useStore((s) => s.pruneCache);
  const setShowShortcuts = useStore((s) => s.setShowShortcuts);
  const clearTags = useStore((s) => s.clearTags);
  const exportPack = useStore((s) => s.exportPack);
  const tags = useStore((s) => s.tags);

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
              disabled={tags.length === 0}
              onClick={async () => {
                const names = tags.map(([n]) => n);
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

          <div className="sidebar-h">Downloading</div>
          <ImportSettings />

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
