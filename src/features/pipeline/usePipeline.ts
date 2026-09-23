import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import type { AppError, Defect, PipelineState, ProcEvent, SizeUsage, TestSuite } from "../../lib/bindings";
import { renderAppError } from "../../lib/errors";
import {
  pipelineBuild,
  pipelineCheck,
  pipelineRunTarget,
  pipelineState as fetchPipelineState,
  pipelineStop,
  pipelineTest,
  pipelineUpload,
} from "../../lib/ipc";

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

/** `NFR-P3`: "the log pane virtualises and caps at a configurable 50 000 lines." Falls back
 * to `GlobalSettings.logs.maxLines`'s own default (see `core::settings::LogSettings`) until
 * the caller has that setting loaded. */
const DEFAULT_MAX_LOG_LINES = 50_000;

interface PipelineStatePayload {
  workspace: string;
  state: PipelineState;
  since: string;
}

export function usePipeline(workspaceId: string, maxLines: number = DEFAULT_MAX_LOG_LINES) {
  const [state, setState] = useState<PipelineState | null>(null);
  const [lines, setLines] = useState<LogLineView[]>([]);
  const [defects, setDefects] = useState<Defect[]>([]);
  const [testSuites, setTestSuites] = useState<TestSuite[]>([]);
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
        setTestSuites([]);
        setSize(null);
        setStage(null);
        setRunning(true);
        break;
      case "lines":
        setLines((prev) => {
          const next = [...prev, ...ev.data.lines.map((l) => ({ stream: l.stream, text: l.text }))];
          return next.length > maxLines ? next.slice(next.length - maxLines) : next;
        });
        break;
      case "defect":
        setDefects((prev) => [...prev, ev.data.defect]);
        break;
      case "testResult":
        setTestSuites((prev) => [...prev, ev.data.suite]);
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
  }, [maxLines]);

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
  const check = useCallback(() => run(() => pipelineCheck(workspaceId, handleEvent)), [run, workspaceId, handleEvent]);
  const test = useCallback(() => run(() => pipelineTest(workspaceId, handleEvent)), [run, workspaceId, handleEvent]);

  const stop = useCallback(() => {
    if (procIdRef.current) void pipelineStop(procIdRef.current);
  }, []);

  return { state, lines, defects, testSuites, size, stage, running, procId, error, build, upload, runTarget, check, test, stop };
}
