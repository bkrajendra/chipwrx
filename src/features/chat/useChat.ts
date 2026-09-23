import { useCallback, useEffect, useRef, useState } from "react";
import type { AppError, ChatEvent, TurnRecord } from "../../lib/bindings";
import { renderAppError } from "../../lib/errors";
import { claudeHistory, claudeNewSession, claudeSendTurn, claudeStopTurn } from "../../lib/ipc";
import { applyEvent, toTurnRecord, type LiveTurn } from "./blocks";

function isAppError(e: unknown): e is AppError {
  return typeof e === "object" && e !== null && "code" in e;
}

export function describeSendError(e: unknown): string {
  if (isAppError(e)) {
    const { title, message } = renderAppError(e);
    return `${title}: ${message}`;
  }
  return String(e);
}

/** `stopGraceMs`'s local (non-persisted) analogue for the chat pane — `PipelineSettings.
 * stopGraceMs` (`DATA-MODEL.md` §3) is a build-pipeline setting, not a chat one; M3 doesn't
 * add a dedicated persisted setting for this, so the default matches that one's default. */
const STOP_GRACE_MS = 3000;
const HISTORY_PAGE_SIZE = 50;

export function useChat(workspaceId: string) {
  const [history, setHistory] = useState<TurnRecord[]>([]);
  const [loadingHistory, setLoadingHistory] = useState(true);
  const [live, setLive] = useState<LiveTurn | null>(null);
  const [error, setError] = useState<string | null>(null);
  const stopTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // `NFR-P2`: "text deltas are batched per animation frame, not per event" — a 200KB
  // streamed response can arrive as hundreds of small `textDelta` events; applying each one
  // as its own `setLive` call would re-render that many times. Every event in a frame is
  // queued here and folded into one `setLive` call per `requestAnimationFrame` instead.
  const pendingEvents = useRef<ChatEvent[]>([]);
  const rafId = useRef<number | null>(null);

  const flushPendingEvents = useCallback(() => {
    rafId.current = null;
    const events = pendingEvents.current;
    pendingEvents.current = [];
    if (events.length === 0) return;
    setLive((prev) => {
      if (!prev) return prev;
      let next = prev;
      for (const ev of events) next = applyEvent(next, ev);
      if (prev.running && !next.running && stopTimer.current) {
        clearTimeout(stopTimer.current);
        stopTimer.current = null;
      }
      return next;
    });
  }, []);

  const queueEvent = useCallback(
    (event: ChatEvent) => {
      pendingEvents.current.push(event);
      if (rafId.current === null) {
        rafId.current = requestAnimationFrame(flushPendingEvents);
      }
    },
    [flushPendingEvents],
  );

  useEffect(() => {
    return () => {
      if (rafId.current !== null) cancelAnimationFrame(rafId.current);
    };
  }, []);

  const reloadHistory = useCallback(async () => {
    setLoadingHistory(true);
    try {
      setHistory(await claudeHistory(workspaceId, HISTORY_PAGE_SIZE));
    } finally {
      setLoadingHistory(false);
    }
  }, [workspaceId]);

  useEffect(() => {
    void reloadHistory();
  }, [reloadHistory]);

  useEffect(() => {
    return () => {
      if (stopTimer.current) clearTimeout(stopTimer.current);
    };
  }, []);

  const send = useCallback(
    async (prompt: string, attachments: string[] = []) => {
      if (live?.running) return;
      setError(null);

      // `live` always represents "the current or most-recent turn," never both a history
      // entry *and* the live view at the same time — rendering it as both (the previous
      // behavior: archiving into `history` the instant a turn finished, while leaving
      // `live` populated) duplicated the just-finished turn on screen. So the previous
      // turn — if any, and by now always finished, since `running` was just checked above
      // — is archived here, right as the *next* one starts, not when it completes.
      if (live) {
        const finished = live;
        setHistory((h) => [...h, toTurnRecord(finished)]);
      }

      const started: LiveTurn = {
        turnId: "",
        sessionId: live?.sessionId ?? null,
        prompt,
        startedAt: new Date().toISOString(),
        blocks: [],
        running: true,
      };
      setLive(started);

      try {
        const turnId = await claudeSendTurn({ workspace: workspaceId, prompt, attachments, policy: null, model: null }, queueEvent);
        setLive((prev) => (prev ? { ...prev, turnId } : prev));
      } catch (e) {
        setError(describeSendError(e));
        setLive(null);
      }
    },
    [live, workspaceId, queueEvent],
  );

  const stop = useCallback(() => {
    setLive((prev) => {
      if (!prev || !prev.turnId || !prev.running) return prev;
      void claudeStopTurn(prev.turnId, false);
      stopTimer.current = setTimeout(() => {
        void claudeStopTurn(prev.turnId, true);
      }, STOP_GRACE_MS);
      return prev;
    });
  }, []);

  const newSession = useCallback(async () => {
    await claudeNewSession(workspaceId);
    setHistory([]);
    setLive(null);
  }, [workspaceId]);

  return { history, live, loadingHistory, error, send, stop, newSession, reloadHistory };
}
