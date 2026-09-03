import { useEffect, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

type Phase = { kind: "idle" | "checking" | "found" | "downloading" | "ready" | "error"; text?: string };

/// Shows nothing until there is actually an update, so it stays out of the way.
export function Updater({ manual, onDone }: { manual: boolean; onDone: () => void }) {
  const [update, setUpdate] = useState<Update | null>(null);
  const [phase, setPhase] = useState<Phase>({ kind: "idle" });
  const [pct, setPct] = useState(0);

  const look = async (announceNothing: boolean) => {
    setPhase({ kind: "checking" });
    try {
      const found = await check();
      if (found) {
        setUpdate(found);
        setPhase({ kind: "found" });
      } else {
        setPhase(announceNothing ? { kind: "error", text: "already up to date" } : { kind: "idle" });
      }
    } catch (e) {
      // A failed check must never block using the app, so it stays quiet
      // unless the user asked for it.
      setPhase(announceNothing ? { kind: "error", text: `update check failed: ${e}` } : { kind: "idle" });
    } finally {
      onDone();
    }
  };

  useEffect(() => {
    void look(false);
  }, []);

  useEffect(() => {
    if (manual) void look(true);
  }, [manual]);

  const install = async () => {
    if (!update) return;
    setPhase({ kind: "downloading" });
    let total = 0;
    let got = 0;
    try {
      await update.downloadAndInstall((e) => {
        if (e.event === "Started") total = e.data.contentLength ?? 0;
        if (e.event === "Progress") {
          got += e.data.chunkLength;
          if (total) setPct(Math.round((got / total) * 100));
        }
      });
      setPhase({ kind: "ready" });
    } catch (e) {
      setPhase({ kind: "error", text: `update failed: ${e}` });
    }
  };

  if (phase.kind === "idle" || phase.kind === "checking") return null;

  return (
    <div className="updater">
      {phase.kind === "found" && update && (
        <>
          <span>
            Version <b>{update.version}</b> is available.
          </span>
          <button onClick={() => void install()}>Update</button>
          <button onClick={() => setPhase({ kind: "idle" })}>Later</button>
        </>
      )}
      {phase.kind === "downloading" && <span>Downloading… {pct ? `${pct}%` : ""}</span>}
      {phase.kind === "ready" && (
        <>
          <span>Update installed.</span>
          <button onClick={() => void relaunch()}>Restart now</button>
        </>
      )}
      {phase.kind === "error" && (
        <>
          <span className="dim">{phase.text}</span>
          <button onClick={() => setPhase({ kind: "idle" })}>Dismiss</button>
        </>
      )}
    </div>
  );
}
