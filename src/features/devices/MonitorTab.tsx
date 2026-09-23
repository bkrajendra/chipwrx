// Canvas "Monitor" tab (`FR-UI-3`, `FR-DEV-5/6/7`) — the in-app serial terminal. Device
// selection itself lives in the sidebar's Hardware card (`TelemetryCard`); this only needs
// to know whether a port is currently selected.

import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { lazy, Suspense, useCallback, useMemo, useState } from "react";
import { useMonitor } from "./useMonitor";

const MonitorXterm = lazy(() => import("./MonitorXterm").then((m) => ({ default: m.MonitorXterm })));

const secondaryButton =
  "rounded border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800";
const primaryButton = "rounded bg-neutral-900 px-2.5 py-1 text-xs font-medium text-neutral-50 disabled:opacity-40 dark:bg-neutral-100 dark:text-neutral-900";

export function MonitorTab({
  workspaceId,
  hasSelectedPort,
  maxLines,
  maxBytes,
  onSendToClaude,
}: {
  workspaceId: string;
  hasSelectedPort: boolean;
  maxLines: number;
  maxBytes: number;
  onSendToClaude: (text: string) => void;
}) {
  const monitor = useMonitor(workspaceId, maxLines, maxBytes);
  const [timestamps, setTimestamps] = useState(false);
  const [autoscroll, setAutoscroll] = useState(true);
  const [filterText, setFilterText] = useState("");
  const [sendText, setSendText] = useState("");
  const [selection, setSelection] = useState("");

  const filter = useMemo(() => {
    if (!filterText) return null;
    try {
      return new RegExp(filterText, "i");
    } catch {
      return null;
    }
  }, [filterText]);

  const handleSaveLog = useCallback(async () => {
    const path = await saveDialog({ defaultPath: "monitor-log.txt" });
    if (path) await monitor.saveLog(path);
  }, [monitor]);

  const handleSend = useCallback(() => {
    if (!sendText) return;
    void monitor.send(sendText);
    setSendText("");
  }, [sendText, monitor]);

  return (
    <div className="flex h-full flex-col">
      <div className="flex flex-wrap items-center gap-2 border-b border-neutral-200 px-4 py-2 dark:border-neutral-800">
        {!monitor.connected ? (
          <button type="button" onClick={() => void monitor.start()} disabled={!hasSelectedPort} className={primaryButton}>
            Start monitor
          </button>
        ) : (
          <button type="button" onClick={() => void monitor.stop()} className={secondaryButton}>
            Stop
          </button>
        )}
        <button type="button" onClick={monitor.clear} className={secondaryButton}>
          Clear
        </button>
        <button type="button" onClick={() => void handleSaveLog()} className={secondaryButton}>
          Save log
        </button>
        <button type="button" onClick={() => void monitor.openExternal()} disabled={!hasSelectedPort} className={secondaryButton}>
          Open in terminal
        </button>
        <label className="flex items-center gap-1 text-xs">
          <input type="checkbox" checked={autoscroll} onChange={(e) => setAutoscroll(e.target.checked)} />
          Autoscroll
        </label>
        <label className="flex items-center gap-1 text-xs">
          <input type="checkbox" checked={timestamps} onChange={(e) => setTimestamps(e.target.checked)} />
          Timestamps
        </label>
        <input
          value={filterText}
          onChange={(e) => setFilterText(e.target.value)}
          placeholder="Filter (regex)"
          aria-label="Filter monitor output"
          className="ml-auto w-40 rounded border border-neutral-300 bg-transparent px-2 py-1 text-xs dark:border-neutral-700"
        />
      </div>

      {monitor.status && <p className="border-b border-neutral-200 px-4 py-1.5 text-xs text-amber-600 dark:border-neutral-800 dark:text-amber-400">{monitor.status}</p>}
      {monitor.error && <p className="border-b border-neutral-200 px-4 py-1.5 text-xs text-red-500 dark:border-neutral-800">{monitor.error}</p>}

      <div className="min-h-0 flex-1">
        <Suspense fallback={<p className="px-4 py-2 text-xs text-neutral-500 dark:text-neutral-400">Loading terminal…</p>}>
          <MonitorXterm lines={monitor.lines} timestamps={timestamps} filter={filter} autoscroll={autoscroll} onSelectionChange={setSelection} />
        </Suspense>
      </div>

      {selection && (
        <div className="border-t border-neutral-200 px-4 py-2 dark:border-neutral-800">
          <button type="button" onClick={() => onSendToClaude("```\n" + selection + "\n```")} className={secondaryButton}>
            Send selection to Claude
          </button>
        </div>
      )}

      <div className="flex gap-2 border-t border-neutral-200 px-4 py-2 dark:border-neutral-800">
        <input
          value={sendText}
          onChange={(e) => setSendText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") handleSend();
          }}
          disabled={!monitor.connected}
          placeholder="Send to device…"
          aria-label="Send to device"
          className="flex-1 rounded border border-neutral-300 bg-transparent px-2 py-1 text-xs disabled:opacity-40 dark:border-neutral-700"
        />
        <button type="button" onClick={handleSend} disabled={!monitor.connected || !sendText} className={secondaryButton}>
          Send
        </button>
      </div>
    </div>
  );
}
