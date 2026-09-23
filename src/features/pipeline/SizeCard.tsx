// Sidebar "Firmware size card" (`FR-UI-2`).

import type { SizeUsage } from "../../lib/bindings";

function formatBytes(n: number): string {
  return n >= 1024 * 1024 ? `${(n / (1024 * 1024)).toFixed(2)} MB` : `${(n / 1024).toFixed(1)} KB`;
}

function SizeBar({ label, used, total, delta }: { label: string; used: number; total: number; delta: number | null }) {
  const pct = total > 0 ? Math.min(100, (used / total) * 100) : 0;
  return (
    <div className="text-xs">
      <div className="mb-0.5 flex justify-between">
        <span className="font-medium">{label}</span>
        <span className="text-neutral-500 dark:text-neutral-400">
          {formatBytes(used)} / {formatBytes(total)} ({pct.toFixed(1)}%)
          {delta !== null && delta !== 0 && (
            <span className={delta > 0 ? " text-amber-600 dark:text-amber-400" : " text-emerald-600 dark:text-emerald-400"}>
              {" "}
              {delta > 0 ? "+" : ""}
              {formatBytes(delta)}
            </span>
          )}
        </span>
      </div>
      <div className="h-1.5 w-full overflow-hidden rounded bg-neutral-200 dark:bg-neutral-800" role="progressbar" aria-valuenow={Math.round(pct)} aria-valuemin={0} aria-valuemax={100} aria-label={`${label} usage`}>
        <div className="h-full bg-neutral-600 dark:bg-neutral-400" style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}

export function SizeCard({ size }: { size: SizeUsage | null }) {
  if (!size) return null;
  return (
    <div className="space-y-2 border-b border-neutral-200 p-3 dark:border-neutral-800">
      <h3 className="text-xs font-semibold text-neutral-500 dark:text-neutral-400">Firmware size</h3>
      <SizeBar label="RAM" used={size.ramUsed} total={size.ramTotal} delta={size.ramDelta} />
      <SizeBar label="Flash" used={size.flashUsed} total={size.flashTotal} delta={size.flashDelta} />
    </div>
  );
}
