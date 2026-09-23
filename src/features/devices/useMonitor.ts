// The in-app serial monitor's frontend state (`FR-DEV-5`). Buffers decoded `Data` chunks
// into complete lines (split on `\n`) so filtering, timestamps, and "Send to Claude" all
// work against whole lines rather than arbitrary byte fragments.

import { useCallback, useEffect, useRef, useState } from "react";
import type { AppError, MonitorEvent } from "../../lib/bindings";
import { renderAppError } from "../../lib/errors";
import { monitorOpenExternal, monitorSaveLog, monitorSend, monitorStart, monitorStop } from "../../lib/ipc";

export interface MonitorLine {
  text: string;
  tsMs: number;
}

/** Client-side display cap defaults — separate from, and independent of, the backend's own
 * byte/line-capped ring buffer (`NFR-P3`), which is what `monitor_save_log` actually
 * writes. Callers pass `GlobalSettings.monitor.{maxLines,maxBytes}` to keep the *visible*
 * buffer's cap in sync with the same setting that governs the backend one; these are just
 * the fallback before that first settings read resolves. */
const DEFAULT_MAX_LINES = 5000;
const DEFAULT_MAX_BYTES = 2_000_000;

function capLines(lines: MonitorLine[], maxLines: number, maxBytes: number): MonitorLine[] {
  let start = Math.max(0, lines.length - maxLines);
  let bytes = 0;
  for (let i = lines.length - 1; i >= start; i--) {
    bytes += lines[i].text.length;
    if (bytes > maxBytes) {
      start = i + 1;
      break;
    }
  }
  return start === 0 ? lines : lines.slice(start);
}

function isAppError(e: unknown): e is AppError {
  return typeof e === "object" && e !== null && "code" in e;
}

function describe(e: unknown): string {
  return isAppError(e) ? renderAppError(e).message : String(e);
}

export function useMonitor(workspaceId: string, maxLines: number = DEFAULT_MAX_LINES, maxBytes: number = DEFAULT_MAX_BYTES) {
  const [connected, setConnected] = useState(false);
  const [port, setPort] = useState<string | null>(null);
  const [baud, setBaud] = useState<number | null>(null);
  const [lines, setLines] = useState<MonitorLine[]>([]);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const partialRef = useRef("");

  const handleEvent = useCallback((ev: MonitorEvent) => {
    switch (ev.type) {
      case "opened":
        setConnected(true);
        setPort(ev.data.port);
        setBaud(ev.data.baud);
        setStatus(null);
        break;
      case "data": {
        const combined = partialRef.current + ev.data.chunk;
        const parts = combined.split("\n");
        partialRef.current = parts.pop() ?? "";
        if (parts.length > 0) {
          setLines((prev) => capLines([...prev, ...parts.map((text) => ({ text, tsMs: ev.data.tsMs }))], maxLines, maxBytes));
        }
        break;
      }
      case "preempted":
        setConnected(false);
        setStatus(`Paused — port taken by ${ev.data.by}`);
        break;
      case "reattached":
        setConnected(true);
        setPort(ev.data.port);
        setStatus(`Reconnected on ${ev.data.port}`);
        break;
      case "closed":
        setConnected(false);
        setStatus(ev.data.reason);
        break;
      case "error":
        setConnected(false);
        setError(describe(ev.data.error));
        break;
      default:
        break;
    }
  }, [maxLines, maxBytes]);

  const start = useCallback(async () => {
    setError(null);
    setStatus(null);
    setLines([]);
    partialRef.current = "";
    try {
      await monitorStart(workspaceId, handleEvent);
    } catch (e) {
      setError(describe(e));
    }
  }, [workspaceId, handleEvent]);

  const stop = useCallback(async () => {
    try {
      await monitorStop(workspaceId);
    } catch (e) {
      setError(describe(e));
    } finally {
      setConnected(false);
    }
  }, [workspaceId]);

  const send = useCallback(
    async (text: string) => {
      try {
        await monitorSend(workspaceId, text);
      } catch (e) {
        setError(describe(e));
      }
    },
    [workspaceId],
  );

  const clear = useCallback(() => setLines([]), []);

  const saveLog = useCallback(
    async (path: string) => {
      try {
        await monitorSaveLog(workspaceId, path);
      } catch (e) {
        setError(describe(e));
      }
    },
    [workspaceId],
  );

  const openExternal = useCallback(async () => {
    try {
      await monitorOpenExternal(workspaceId);
    } catch (e) {
      setError(describe(e));
    } finally {
      setConnected(false);
    }
  }, [workspaceId]);

  // Best-effort: release the port when the panel/workspace goes away. A monitor the user
  // never started is a harmless no-op stop on the backend.
  useEffect(() => {
    return () => {
      void monitorStop(workspaceId);
    };
  }, [workspaceId]);

  return { connected, port, baud, lines, status, error, start, stop, send, clear, saveLog, openExternal };
}
