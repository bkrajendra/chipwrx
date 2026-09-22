// `FR-UI-4`: "A persistent pipeline strip sits above the canvas showing the state machine
// with the current step highlighted, elapsed time, and a Stop button when anything runs."
// M8 owns the full canvas/strip shell; this is the compact header-mounted version that
// exists until then.

import { useEffect, useState } from "react";
import type { PipelineState } from "../../lib/bindings";

const STEP_LABEL: Record<PipelineState["step"], string> = {
  idle: "Idle",
  building: "Building…",
  buildOk: "Build OK",
  uploading: "Uploading…",
  failed: "Build failed",
  monitoring: "Monitoring",
};

const STEP_COLOR: Record<PipelineState["step"], string> = {
  idle: "text-neutral-500 dark:text-neutral-400",
  building: "text-amber-600 dark:text-amber-400",
  uploading: "text-amber-600 dark:text-amber-400",
  buildOk: "text-emerald-600 dark:text-emerald-400",
  failed: "text-red-600 dark:text-red-400",
  monitoring: "text-blue-600 dark:text-blue-400",
};

function useElapsedSeconds(since: string, active: boolean): number {
  const [elapsed, setElapsed] = useState(0);
  useEffect(() => {
    if (!active) {
      setElapsed(0);
      return;
    }
    const start = new Date(since).getTime();
    const tick = () => setElapsed(Math.max(0, Math.floor((Date.now() - start) / 1000)));
    tick();
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, [since, active]);
  return elapsed;
}

export function PipelineStrip({
  state,
  onStop,
  onOpen,
}: {
  state: PipelineState | null;
  onStop: () => void;
  onOpen: () => void;
}) {
  const active = state?.step === "building" || state?.step === "uploading";
  const elapsed = useElapsedSeconds(state?.since ?? new Date().toISOString(), active);

  if (!state) return null;

  return (
    <button
      type="button"
      onClick={onOpen}
      className="flex items-center gap-2 rounded border border-neutral-300 px-2.5 py-1 text-xs dark:border-neutral-700"
    >
      <span className={STEP_COLOR[state.step]}>{STEP_LABEL[state.step]}</span>
      {active && <span className="text-neutral-400">{elapsed}s</span>}
      {active && (
        <span
          role="button"
          tabIndex={0}
          onClick={(e) => {
            e.stopPropagation();
            onStop();
          }}
          className="text-red-500 hover:underline"
        >
          Stop
        </span>
      )}
    </button>
  );
}
