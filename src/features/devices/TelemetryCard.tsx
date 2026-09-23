// Sidebar "Hardware telemetry card" (`FR-UI-2`). Device picker + `FR-DEV-3` telemetry
// summary + the `FR-DEV-9` "new board detected" toast.

import type { SerialDevice, Telemetry } from "../../lib/bindings";
import type { UseDeviceTelemetry } from "./useDeviceTelemetry";

function describeDevice(d: SerialDevice): string {
  return `${d.port} — ${d.knownBridge ?? d.description}`;
}

// `NFR-A2`: "status conveyed by icon + text, never colour alone" — every telemetry value
// below is plain text, not a gauge, so this component already satisfies that by construction.
function TelemetrySummary({ telemetry }: { telemetry: Telemetry }) {
  if (!telemetry.connected) {
    return <p className="text-xs text-neutral-500 dark:text-neutral-400">{telemetry.unavailableReason ?? "Not available for this board."}</p>;
  }
  return (
    <dl className="space-y-0.5 text-xs text-neutral-700 dark:text-neutral-300">
      <div className="flex justify-between gap-2">
        <dt className="text-neutral-500 dark:text-neutral-400">Chip</dt>
        <dd className="text-right">{telemetry.chip ?? telemetry.board?.name ?? telemetry.adapter}</dd>
      </div>
      {telemetry.mac && (
        <div className="flex justify-between gap-2">
          <dt className="text-neutral-500 dark:text-neutral-400">MAC</dt>
          <dd className="font-mono">{telemetry.mac}</dd>
        </div>
      )}
      {telemetry.flashSize && (
        <div className="flex justify-between gap-2">
          <dt className="text-neutral-500 dark:text-neutral-400">Flash</dt>
          <dd>
            {telemetry.flashSize}
            {telemetry.flashVendor ? ` (${telemetry.flashVendor})` : ""}
          </dd>
        </div>
      )}
      {telemetry.board && !telemetry.chip && (
        <div className="flex justify-between gap-2">
          <dt className="text-neutral-500 dark:text-neutral-400">MCU</dt>
          <dd>
            {telemetry.board.mcu} @ {Math.round(telemetry.board.fcpu / 1_000_000)}MHz
          </dd>
        </div>
      )}
    </dl>
  );
}

export function TelemetryCard({ device }: { device: UseDeviceTelemetry }) {
  const { devices, selectedPort, telemetry, telemetryLoading, selectPort, refreshTelemetry, justConnected, dismissJustConnected } = device;

  return (
    <div className="space-y-2 border-b border-neutral-200 p-3 dark:border-neutral-800">
      <h3 className="text-xs font-semibold text-neutral-500 dark:text-neutral-400">Hardware</h3>

      {justConnected && (
        <div className="rounded border border-amber-300 bg-amber-50 p-2 text-[11px] dark:border-amber-800 dark:bg-amber-950">
          <p className="mb-1.5">
            New board on {justConnected.port} ({justConnected.knownBridge}) — use it?
          </p>
          <div className="flex gap-2">
            <button
              type="button"
              onClick={() => {
                void selectPort(justConnected.port);
                dismissJustConnected();
              }}
              className="rounded bg-neutral-900 px-2 py-0.5 font-medium text-neutral-50 dark:bg-neutral-100 dark:text-neutral-900"
            >
              Use it
            </button>
            <button type="button" onClick={dismissJustConnected} className="rounded border border-neutral-300 px-2 py-0.5 dark:border-neutral-700">
              Dismiss
            </button>
          </div>
        </div>
      )}

      <select
        aria-label="Serial device"
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

      {telemetry && <TelemetrySummary telemetry={telemetry} />}

      <button
        type="button"
        onClick={() => void refreshTelemetry()}
        disabled={telemetryLoading}
        className="w-full rounded border border-neutral-300 px-2 py-1 text-xs font-medium hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800"
      >
        {telemetryLoading ? "Refreshing…" : "Refresh"}
      </button>
    </div>
  );
}
