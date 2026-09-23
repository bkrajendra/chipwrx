// Sidebar "Control deck" (`FR-UI-2`): Build, Upload, Monitor, plus an overflow menu of
// extra targets. `FR-UI-6`: every long-running action here is cancellable and shows
// elapsed time via the pipeline strip above the canvas, not duplicated here.

import { useEffect, useState } from "react";
import type { PipelineState } from "../../lib/bindings";
import { pipelineTargets } from "../../lib/ipc";

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
        aria-haspopup="menu"
        aria-expanded={open}
        className="w-full rounded border border-neutral-300 px-2 py-1 text-xs font-medium hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800"
      >
        More targets ▾
      </button>
      {open && (
        <div role="menu" className="absolute left-0 top-full z-10 mt-1 w-full rounded border border-neutral-300 bg-white py-1 text-xs shadow-lg dark:border-neutral-700 dark:bg-neutral-900">
          {targets === null && <p className="px-2 py-1 text-neutral-500 dark:text-neutral-400">Loading…</p>}
          {targets?.map((t) => (
            <button
              key={t}
              type="button"
              role="menuitem"
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

export function ControlDeck({
  workspaceId,
  state,
  running,
  onBuild,
  onUpload,
  onRunTarget,
  onStop,
  onOpenMonitor,
}: {
  workspaceId: string;
  state: PipelineState | null;
  running: boolean;
  onBuild: () => void;
  onUpload: () => void;
  onRunTarget: (target: string) => void;
  onStop: () => void;
  onOpenMonitor: () => void;
}) {
  return (
    <div className="space-y-2 border-b border-neutral-200 p-3 dark:border-neutral-800">
      <h3 className="text-xs font-semibold text-neutral-500 dark:text-neutral-400">Control</h3>
      <div className="grid grid-cols-2 gap-1.5">
        <button
          type="button"
          onClick={onBuild}
          disabled={running}
          className="rounded bg-neutral-900 px-2.5 py-1.5 text-xs font-medium text-neutral-50 disabled:opacity-40 dark:bg-neutral-100 dark:text-neutral-900"
          title="Build (Cmd/Ctrl+B)"
        >
          Build
        </button>
        <button
          type="button"
          onClick={onUpload}
          disabled={running || state?.step !== "buildOk"}
          title={state?.step !== "buildOk" ? "Build the current code successfully before uploading" : "Upload (Cmd/Ctrl+U)"}
          className="rounded border border-neutral-300 px-2.5 py-1.5 text-xs font-medium hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800"
        >
          Upload
        </button>
      </div>
      <button
        type="button"
        onClick={onOpenMonitor}
        className="w-full rounded border border-neutral-300 px-2.5 py-1.5 text-xs font-medium hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
        title="Monitor (Cmd/Ctrl+M)"
      >
        Monitor
      </button>
      <TargetsMenu workspaceId={workspaceId} disabled={running} onRun={onRunTarget} />
      {running && (
        <button
          type="button"
          onClick={onStop}
          className="w-full rounded border border-red-400 px-2.5 py-1.5 text-xs font-medium text-red-600 hover:bg-red-50 dark:hover:bg-red-950"
          title="Stop (Esc)"
        >
          Stop
        </button>
      )}
    </div>
  );
}
