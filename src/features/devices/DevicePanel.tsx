// The Devices side panel: picker + telemetry card (`FR-DEV-2/3`) and the in-app serial
// monitor (`FR-DEV-5/6/7`). Toggled from the Chat header, same pattern as
// `PipelinePanel`/`ChangesPanel`.

import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { lazy, Suspense, useCallback, useEffect, useMemo, useState } from "react";
import type { SerialDevice, Telemetry } from "../../lib/bindings";
import { deviceSelect, deviceTelemetry } from "../../lib/ipc";
import { useDeviceList } from "./useDeviceList";
import { useMonitor } from "./useMonitor";

const MonitorXterm = lazy(() => import("./MonitorXterm").then((m) => ({ default: m.MonitorXterm })));

function describeDevice(d: SerialDevice): string {
  return `${d.port} — ${d.knownBridge ?? d.description}`;
}

function TelemetryCard({ telemetry }: { telemetry: Telemetry }) {
  if (!telemetry.connected) {
    return <p className="text-xs text-neutral-500 dark:text-neutral-400">{telemetry.unavailableReason ?? "Not available for this board."}</p>;
  }
  return (
    <div className="space-y-0.5 text-xs text-neutral-700 dark:text-neutral-300">
      <p className="font-medium">{telemetry.chip ?? telemetry.board?.name ?? telemetry.adapter}</p>
      {telemetry.mac && <p>MAC {telemetry.mac}</p>}
      {telemetry.flashSize && (
        <p>
          Flash {telemetry.flashSize}
          {telemetry.flashVendor ? ` (vendor ${telemetry.flashVendor})` : ""}
        </p>
      )}
      {telemetry.board && !telemetry.chip && (
        <p>
          {telemetry.board.mcu} @ {Math.round(telemetry.board.fcpu / 1_000_000)}MHz
        </p>
      )}
    </div>
  );
}

const secondaryButton =
  "rounded border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800";
const primaryButton =
  "rounded bg-neutral-900 px-2.5 py-1 text-xs font-medium text-neutral-50 disabled:opacity-40 dark:bg-neutral-100 dark:text-neutral-900";

export function DevicePanel({
  workspaceId,
  onClose,
  onSendToClaude,
}: {
  workspaceId: string;
  onClose: () => void;
  onSendToClaude: (text: string) => void;
}) {
  const { devices, justConnected, dismissJustConnected, changeCount } = useDeviceList();
  const monitor = useMonitor(workspaceId);
  const [selectedPort, setSelectedPort] = useState<string | null>(null);
  const [telemetry, setTelemetry] = useState<Telemetry | null>(null);
  const [telemetryLoading, setTelemetryLoading] = useState(false);
  const [timestamps, setTimestamps] = useState(false);
  const [autoscroll, setAutoscroll] = useState(true);
  const [filterText, setFilterText] = useState("");
  const [sendText, setSendText] = useState("");
  const [selection, setSelection] = useState("");

  const refreshTelemetry = useCallback(async () => {
    setTelemetryLoading(true);
    try {
      const t = await deviceTelemetry(workspaceId, true);
      setTelemetry(t);
      if (t.port) setSelectedPort(t.port);
    } finally {
      setTelemetryLoading(false);
    }
  }, [workspaceId]);

  useEffect(() => {
    void refreshTelemetry();
    // Re-checks after every `device://changed` too — picks up a silent `FR-DEV-2` sticky
    // rebind (backend-driven, e.g. this workspace's board reappearing on a new port while
    // this panel wasn't the one that noticed it).
  }, [refreshTelemetry, changeCount]);

  const selectPort = useCallback(
    async (port: string) => {
      setSelectedPort(port);
      await deviceSelect(workspaceId, port);
      void refreshTelemetry();
    },
    [workspaceId, refreshTelemetry],
  );

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
      <header className="flex items-center justify-between gap-2 border-b border-neutral-200 px-4 py-2 dark:border-neutral-800">
        <span className="text-sm font-medium">Device</span>
        <button type="button" onClick={onClose} className="text-xs text-neutral-500 hover:underline dark:text-neutral-400">
          Close
        </button>
      </header>

      {justConnected && (
        <div className="flex items-center justify-between gap-2 border-b border-amber-300 bg-amber-50 px-4 py-2 text-xs dark:border-amber-800 dark:bg-amber-950">
          <span>
            New board detected on {justConnected.port} ({justConnected.knownBridge}) — use it?
          </span>
          <div className="flex shrink-0 gap-2">
            <button
              type="button"
              onClick={() => {
                void selectPort(justConnected.port);
                dismissJustConnected();
              }}
              className={primaryButton}
            >
              Use it
            </button>
            <button type="button" onClick={dismissJustConnected} className={secondaryButton}>
              Dismiss
            </button>
          </div>
        </div>
      )}

      <div className="space-y-2 border-b border-neutral-200 px-4 py-2 dark:border-neutral-800">
        <select
          value={selectedPort ?? ""}
          onChange={(e) => void selectPort(e.target.value)}
          className="w-full rounded border border-neutral-300 bg-white px-2 py-1 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100"
        >
          <option value="" disabled>
            {devices.length === 0 ? "No devices found" : "Select a device…"}
          </option>
          {devices.map((d) => (
            <option key={d.port} value={d.port}>
              {describeDevice(d)}
            </option>
          ))}
        </select>
        {telemetry && <TelemetryCard telemetry={telemetry} />}
        <button type="button" onClick={() => void refreshTelemetry()} disabled={telemetryLoading} className={secondaryButton}>
          {telemetryLoading ? "Refreshing…" : "Refresh"}
        </button>
      </div>

      <div className="flex flex-wrap items-center gap-2 border-b border-neutral-200 px-4 py-2 dark:border-neutral-800">
        {!monitor.connected ? (
          <button type="button" onClick={() => void monitor.start()} disabled={!selectedPort} className={primaryButton}>
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
        <button type="button" onClick={() => void monitor.openExternal()} disabled={!selectedPort} className={secondaryButton}>
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
      </div>

      <div className="border-b border-neutral-200 px-4 py-2 dark:border-neutral-800">
        <input
          value={filterText}
          onChange={(e) => setFilterText(e.target.value)}
          placeholder="Filter (regex)"
          className="w-full rounded border border-neutral-300 bg-transparent px-2 py-1 text-xs dark:border-neutral-700"
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
          className="flex-1 rounded border border-neutral-300 bg-transparent px-2 py-1 text-xs disabled:opacity-40 dark:border-neutral-700"
        />
        <button type="button" onClick={handleSend} disabled={!monitor.connected || !sendText} className={secondaryButton}>
          Send
        </button>
      </div>
    </div>
  );
}
