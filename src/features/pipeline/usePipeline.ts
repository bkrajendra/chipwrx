import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import type { AppError, Defect, PipelineState, ProcEvent, SizeUsage } from "../../lib/bindings";
import { renderAppError } from "../../lib/errors";
import { pipelineBuild, pipelineRunTarget, pipelineState as fetchPipelineState, pipelineStop, pipelineUpload } from "../../lib/ipc";

function isAppError(e: unknown): e is AppError {
  return typeof e === "object" && e !== null && "code" in e;
}

function describe(e: unknown): string {
  return isAppError(e) ? renderAppError(e).message : String(e);
}

export interface LogLineView {
  stream: "stdout" | "stderr";
  text: string;
}

/** Caps in-memory log growth for a single run — `NFR-P3`-adjacent; the real virtualized,
 * persistently-capped Logs pane is M8's job. */
const MAX_BUFFERED_LINES = 5000;

interface PipelineStatePayload {
  workspace: string;
  state: PipelineState;
  since: string;
}

export function usePipeline(workspaceId: string) {
  const [state, setState] = useState<PipelineState | null>(null);
  const [lines, setLines] = useState<LogLineView[]>([]);
  const [defects, setDefects] = useState<Defect[]>([]);
  const [size, setSize] = useState<SizeUsage | null>(null);
  const [stage, setStage] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [procId, setProcId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const procIdRef = useRef<string | null>(null);

  useEffect(() => {
    void fetchPipelineState(workspaceId).then(setState);
    const unlisten = listen<PipelineStatePayload>("pipeline://state", (event) => {
      if (event.payload.workspace === workspaceId) setState(event.payload.state);
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, [workspaceId]);

  const handleEvent = useCallback((ev: ProcEvent) => {
    switch (ev.type) {
      case "started":
        procIdRef.current = ev.data.procId;
        setProcId(ev.data.procId);
        setLines([]);
        setDefects([]);
        setSize(null);
        setStage(null);
        setRunning(true);
        break;
      case "lines":
        setLines((prev) => {
          const next = [...prev, ...ev.data.lines.map((l) => ({ stream: l.stream, text: l.text }))];
          return next.length > MAX_BUFFERED_LINES ? next.slice(next.length - MAX_BUFFERED_LINES) : next;
        });
        break;
      case "defect":
        setDefects((prev) => [...prev, ev.data.defect]);
        break;
      case "size":
        setSize(ev.data.usage);
        break;
      case "stage":
        setStage(ev.data.stage);
        break;
      case "finished":
        setRunning(false);
        break;
      default:
        break;
    }
  }, []);

  const run = useCallback(
    async (action: () => Promise<string>) => {
      if (running) return;
      setError(null);
      try {
        await action();
      } catch (e) {
        setError(describe(e));
      }
    },
    [running],
  );

  const build = useCallback(() => run(() => pipelineBuild(workspaceId, handleEvent)), [run, workspaceId, handleEvent]);
  const upload = useCallback(() => run(() => pipelineUpload(workspaceId, handleEvent)), [run, workspaceId, handleEvent]);
  const runTarget = useCallback(
    (target: string) => run(() => pipelineRunTarget(workspaceId, target, handleEvent)),
    [run, workspaceId, handleEvent],
  );

  const stop = useCallback(() => {
    if (procIdRef.current) void pipelineStop(procIdRef.current);
  }, []);

  return { state, lines, defects, size, stage, running, procId, error, build, upload, runTarget, stop };
}
