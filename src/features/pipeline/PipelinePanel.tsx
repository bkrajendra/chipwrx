import { lazy, Suspense, useEffect, useState } from "react";
import type { Defect } from "../../lib/bindings";
import { pipelineTargets } from "../../lib/ipc";
import { composeFixPrompt } from "./fixPrompt";
import type { usePipeline } from "./usePipeline";

// xterm.js is a meaningful chunk of the bundle and is only needed once the Logs tab is
// actually shown — loaded on demand, same reasoning as the Changes panel's DiffViewer.
const LogsPane = lazy(() => import("./LogsPane").then((m) => ({ default: m.LogsPane })));

const SEVERITY_COLOR: Record<Defect["severity"], string> = {
  error: "text-red-600 dark:text-red-400",
  warning: "text-amber-600 dark:text-amber-400",
  note: "text-neutral-500 dark:text-neutral-400",
};

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
      <div className="h-1.5 w-full overflow-hidden rounded bg-neutral-200 dark:bg-neutral-800">
        <div className="h-full bg-neutral-600 dark:bg-neutral-400" style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}


function TargetsMenu({ workspaceId, disabled, onRun }: { workspaceId: string; disabled: boolean; onRun: (target: string) => void }) {
  const [open, setOpen] = useState(false);
  const [targets, setTargets] = useState<string[] | null>(null);

  useEffect(() => {
    if (open && targets === null) {
      void pipelineTargets(workspaceId, false).then(setTargets);
    }
  }, [open, targets, workspaceId]);

  return (
    <div className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        disabled={disabled}
        className="rounded border border-neutral-300 px-2 py-1 text-xs font-medium hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800"
      >
        More ▾
      </button>
      {open && (
        <div className="absolute right-0 top-full z-10 mt-1 w-40 rounded border border-neutral-300 bg-white py-1 text-xs shadow-lg dark:border-neutral-700 dark:bg-neutral-900">
          {targets === null && <p className="px-2 py-1 text-neutral-400">Loading…</p>}
          {targets?.map((t) => (
            <button
              key={t}
              type="button"
              onClick={() => {
                onRun(t);
                setOpen(false);
              }}
              className="block w-full px-2 py-1 text-left hover:bg-neutral-100 dark:hover:bg-neutral-800"
            >
              {t}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function PipelinePanel({
  workspaceId,
  pipeline,
  onClose,
  onAskClaudeToFix,
}: {
  workspaceId: string;
  pipeline: ReturnType<typeof usePipeline>;
  onClose: () => void;
  onAskClaudeToFix: (prompt: string) => void;
}) {
  const { state, lines, defects, size, running, error, build, upload, runTarget, stop } = pipeline;
  const [tab, setTab] = useState<"logs" | "problems">("logs");

  useEffect(() => {
    if (defects.some((d) => d.severity === "error")) setTab("problems");
  }, [defects]);

  const errorCount = defects.filter((d) => d.severity === "error").length;

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center justify-between gap-2 border-b border-neutral-200 px-4 py-2 dark:border-neutral-800">
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => void build()}
            disabled={running}
            className="rounded bg-neutral-900 px-2.5 py-1 text-xs font-medium text-neutral-50 disabled:opacity-40 dark:bg-neutral-100 dark:text-neutral-900"
          >
            Build
          </button>
          <button
            type="button"
            onClick={() => void upload()}
            disabled={running || state?.step !== "buildOk"}
            title={state?.step !== "buildOk" ? "Build the current code successfully before uploading" : undefined}
            className="rounded border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            Upload
          </button>
          <TargetsMenu workspaceId={workspaceId} disabled={running} onRun={(t) => void runTarget(t)} />
          {running && (
            <button type="button" onClick={stop} className="rounded border border-red-400 px-2.5 py-1 text-xs font-medium text-red-600 hover:bg-red-50 dark:hover:bg-red-950">
              Stop
            </button>
          )}
        </div>
        <button type="button" onClick={onClose} className="text-xs text-neutral-500 hover:underline dark:text-neutral-400">
          Close
        </button>
      </header>

      {error && <p className="border-b border-neutral-200 px-4 py-2 text-xs text-red-500 dark:border-neutral-800">{error}</p>}

      {size && (
        <div className="grid grid-cols-2 gap-4 border-b border-neutral-200 px-4 py-2 dark:border-neutral-800">
          <SizeBar label="RAM" used={size.ramUsed} total={size.ramTotal} delta={size.ramDelta} />
          <SizeBar label="Flash" used={size.flashUsed} total={size.flashTotal} delta={size.flashDelta} />
        </div>
      )}

      {!running && errorCount > 0 && (
        <div className="border-b border-neutral-200 px-4 py-2 dark:border-neutral-800">
          <button
            type="button"
            onClick={() => onAskClaudeToFix(composeFixPrompt(defects, lines))}
            className="rounded bg-red-600 px-2.5 py-1.5 text-xs font-medium text-white hover:bg-red-700"
          >
            Ask Claude to fix these {errorCount} error{errorCount === 1 ? "" : "s"}
          </button>
        </div>
      )}

      <div className="flex gap-1 border-b border-neutral-200 px-4 pt-2 dark:border-neutral-800">
        {(["logs", "problems"] as const).map((t) => (
          <button
            key={t}
            type="button"
            onClick={() => setTab(t)}
            className={`rounded-t px-2.5 py-1 text-xs font-medium capitalize ${
              tab === t ? "bg-neutral-100 dark:bg-neutral-800" : "text-neutral-500 dark:text-neutral-400"
            }`}
          >
            {t}
            {t === "problems" && defects.length > 0 ? ` (${defects.length})` : ""}
          </button>
        ))}
      </div>

      <div className="min-h-0 flex-1">
        {tab === "logs" && (
          <Suspense fallback={<p className="px-4 py-2 text-xs text-neutral-500 dark:text-neutral-400">Loading terminal…</p>}>
            <LogsPane lines={lines} />
          </Suspense>
        )}
        {tab === "problems" && (
          <div className="h-full overflow-y-auto px-4 py-2">
            {defects.length === 0 && <p className="text-xs text-neutral-500 dark:text-neutral-400">No problems.</p>}
            {defects.map((d, i) => (
              <div key={i} className="border-b border-neutral-100 py-1.5 text-xs last:border-0 dark:border-neutral-900">
                <span className={`font-mono font-medium ${SEVERITY_COLOR[d.severity]}`}>
                  {d.file}:{d.line}
                  {d.column ? `:${d.column}` : ""}
                </span>
                <span className="ml-1.5 text-neutral-700 dark:text-neutral-300">{d.message}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
