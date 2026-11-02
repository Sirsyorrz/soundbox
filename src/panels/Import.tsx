import { useState } from "react";
import { isFinished, useStore } from "../state/store";
import type { ImportFormat, Job } from "../types";

const FORMATS: { id: ImportFormat; label: string }[] = [
  { id: "original", label: "Original (no conversion)" },
  { id: "wav", label: "WAV" },
  { id: "flac", label: "FLAC" },
  { id: "mp3", label: "MP3" },
];

const BROWSE = "__browse__";

function joinPath(root: string, rel: string) {
  const sep = root.includes("\\") ? "\\" : "/";
  return root.replace(/[\\/]+$/, "") + sep + rel.split("/").join(sep);
}

function lastSegment(path: string) {
  return (
    path
      .replace(/[\\/]+$/, "")
      .split(/[\\/]/)
      .pop() || path
  );
}

/** Every library folder as an import target, plus the current one if it is not among them. */
export function DestSelect() {
  const roots = useStore((s) => s.roots);
  const folders = useStore((s) => s.folders);
  useStore((s) => s.importDest);
  useStore((s) => s.registry);
  const s = useStore.getState();
  const dest = s.importDestPath();

  const options: { value: string; label: string }[] = [];
  for (const r of roots) {
    options.push({ value: r.path, label: r.label });
    for (const f of folders.filter((f) => f.root === r.id)) {
      const depth = f.path.split("/").length;
      options.push({
        value: joinPath(r.path, f.path),
        label: "\u00a0\u00a0".repeat(depth) + f.path.split("/").pop(),
      });
    }
  }
  if (dest && !options.some((o) => o.value === dest)) {
    options.unshift({ value: dest, label: lastSegment(dest) });
  }

  return (
    <select
      className="dest"
      value={dest ?? ""}
      title={dest ? `Downloads are saved to\n${dest}` : "Add a library folder first"}
      onChange={(e) => {
        if (e.target.value === BROWSE) void s.browseImportDest();
        else s.setImportDest(e.target.value);
      }}
    >
      {!dest && <option value="">no library folder</option>}
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
      <option value={BROWSE}>Other folder</option>
    </select>
  );
}

function percent(j: Job) {
  if (j.done == null || !j.total) return null;
  return Math.min(100, Math.round((j.done / j.total) * 100));
}

function describe(j: Job) {
  const of = j.count && j.count > 1 && j.index ? ` ${j.index}/${j.count}` : "";
  switch (j.state) {
    case "starting":
      return "starting";
    case "updating":
      return "updating yt-dlp";
    case "running": {
      const p = percent(j);
      return p == null ? `downloading${of}` : `${p}%${of}`;
    }
    case "converting":
      return `processing${of}`;
    case "done":
      return j.files ? `${j.files} added` : "nothing new";
    case "failed":
      return "failed";
    case "cancelled":
      return "cancelled";
  }
}

function JobRow({ job }: { job: Job }) {
  const cancel = useStore((s) => s.cancelImport);
  const dismiss = useStore((s) => s.dismissJob);
  const finished = isFinished(job);
  const p = job.state === "running" ? percent(job) : null;
  const tip = [job.title, job.url, job.error, `to ${job.dest}`].filter(Boolean).join("\n");

  return (
    <div className={"job" + (job.state === "failed" ? " bad" : "")} title={tip}>
      <div className="job-line">
        <span className="job-name">{job.title ?? job.url}</span>
        <span className="job-state">{describe(job)}</span>
        <button
          className="x"
          title={finished ? "Dismiss" : "Cancel this download"}
          onClick={() => (finished ? dismiss(job.id) : cancel(job.id))}
        >
          ×
        </button>
      </div>
      {!finished && (
        <div className={"job-bar" + (p == null ? " busy" : "")}>
          <div style={{ width: p == null ? "100%" : `${p}%` }} />
        </div>
      )}
      {job.error && <div className="job-error">{job.error}</div>}
    </div>
  );
}

function SetupProgress() {
  const t = useStore((s) => s.toolProgress);
  if (!t) return null;
  const p = t.total ? Math.round((t.done / t.total) * 100) : null;
  const mb = (t.done / 1048576).toFixed(0);
  return (
    <div className="job">
      <div className="job-line">
        <span className="job-name">getting {t.stage}</span>
        <span className="job-state">{p == null ? `${mb} MB` : `${p}%`}</span>
      </div>
      <div className={"job-bar" + (p == null ? " busy" : "")}>
        <div style={{ width: p == null ? "100%" : `${p}%` }} />
      </div>
    </div>
  );
}

/** The sidebar part: paste a link, press Enter. Everything else is in the dialog. */
export function ImportBar() {
  const [url, setUrl] = useState("");
  const jobs = useStore((s) => s.jobs);
  const start = useStore((s) => s.startImport);
  const setShowImport = useStore((s) => s.setShowImport);
  // Finished successes only show in the dialog; failures stay until dismissed
  // so they are not missed.
  const visible = jobs.filter((j) => !isFinished(j) || j.state === "failed");

  const submit = async () => {
    if (!url.trim()) return;
    if (await start(url)) setUrl("");
  };

  return (
    <>
      <div className="sidebar-h">Download</div>
      <div className="import-row" data-own-keys>
        <input
          value={url}
          placeholder="paste a link"
          title="YouTube, SoundCloud, Bandcamp and most other sites"
          onChange={(e) => setUrl(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void submit();
            if (e.key === "Escape") setUrl("");
          }}
        />
        <button
          title="Download options: format, clip, playlist, history"
          onClick={() => setShowImport(true)}
        >
          …
        </button>
      </div>
      <div className="import-row">
        <span className="dim">to</span>
        <DestSelect />
      </div>
      <SetupProgress />
      {visible.map((j) => (
        <JobRow key={j.id} job={j} />
      ))}
    </>
  );
}

export function ImportDialog() {
  const open = useStore((s) => s.showImport);
  const close = () => useStore.getState().setShowImport(false);
  const jobs = useStore((s) => s.jobs);
  const format = useStore((s) => s.importFormat);
  const setFormat = useStore((s) => s.setImportFormat);
  const playlist = useStore((s) => s.importPlaylist);
  const setPlaylist = useStore((s) => s.setImportPlaylist);
  const start = useStore((s) => s.startImport);
  const clearFinished = useStore((s) => s.clearFinishedJobs);
  const [url, setUrl] = useState("");
  const [clipOn, setClipOn] = useState(false);
  const [from, setFrom] = useState("0:00");
  const [to, setTo] = useState("");

  if (!open) return null;

  const submit = async () => {
    if (!url.trim()) return;
    const clip: [string, string] | null = clipOn ? [from.trim() || "0", to.trim() || "inf"] : null;
    if (await start(url, { clip })) setUrl("");
  };

  return (
    <div className="modal-back" onClick={close}>
      <div className="modal" onClick={(e) => e.stopPropagation()} data-own-keys>
        <div className="modal-h">
          <b>Download from a link</b>
          <span className="spacer" />
          <button onClick={close}>Close</button>
        </div>
        <div className="modal-body">
          <div className="setrow">
            <input
              className="grow"
              autoFocus
              value={url}
              placeholder="https://…"
              onChange={(e) => setUrl(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void submit();
              }}
            />
            <button disabled={!url.trim()} onClick={() => void submit()}>
              Download
            </button>
          </div>

          <div className="setrow">
            <div className="setlabel">
              Save to
              <div className="dim">
                Only folders in this profile's library show up after importing.
              </div>
            </div>
            <DestSelect />
          </div>

          <div className="setrow">
            <div className="setlabel">
              Format
              <div className="dim">
                Original keeps the site's own audio (usually m4a) with no quality loss. Converting
                needs ffmpeg.
              </div>
            </div>
            <select value={format} onChange={(e) => setFormat(e.target.value as ImportFormat)}>
              {FORMATS.map((f) => (
                <option key={f.id} value={f.id}>
                  {f.label}
                </option>
              ))}
            </select>
          </div>

          <div className="setrow">
            <div className="setlabel">
              Whole playlist
              <div className="dim">
                For playlist links, download every entry into a folder named after the playlist.
              </div>
            </div>
            <input
              type="checkbox"
              checked={playlist}
              onChange={(e) => setPlaylist(e.target.checked)}
            />
          </div>

          <div className="setrow">
            <div className="setlabel">
              Clip only
              <div className="dim">
                Just this part, as 90, 1:30 or 1:02:03. Leave the end empty for the rest. Needs
                ffmpeg. Applies to this dialog only.
              </div>
            </div>
            <input type="checkbox" checked={clipOn} onChange={(e) => setClipOn(e.target.checked)} />
          </div>
          {clipOn && (
            <div className="setrow clip">
              <input value={from} onChange={(e) => setFrom(e.target.value)} placeholder="start" />
              <span className="dim">to</span>
              <input value={to} onChange={(e) => setTo(e.target.value)} placeholder="end" />
            </div>
          )}

          <div className="sidebar-h row-h">
            This session
            <span className="spacer" />
            <button disabled={!jobs.some(isFinished)} onClick={clearFinished}>
              Clear finished
            </button>
          </div>
          {jobs.length === 0 && <div className="pad dim">nothing imported yet</div>}
          {jobs
            .slice()
            .reverse()
            .map((j) => (
              <JobRow key={j.id} job={j} />
            ))}
        </div>
      </div>
    </div>
  );
}
