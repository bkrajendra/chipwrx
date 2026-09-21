// The live-turn render model. `ChatEvent`s (IPC-CONTRACT.md §4) get folded into an ordered
// `Block[]` here rather than rendered as a raw event log — FR-CHAT-3: "streaming assistant
// text; a compact card per tool call... expandable result; system/api_retry as a chip; a
// final turn footer."

import type { AppError, ChatEvent, JsonValue, TurnRecord } from "../../lib/bindings";

export type Block =
  | { kind: "text"; blockIndex: number; text: string }
  | {
      kind: "tool";
      toolUseId: string;
      name: string;
      inputPreview: string;
      input: unknown;
      result?: { isError: boolean; summary: string; full: string | null };
    }
  | { kind: "subagent"; parentToolUseId: string; role: string; text: string }
  | { kind: "apiRetry"; attempt: number; maxRetries: number; error: string }
  | { kind: "permissionDenied"; tool: string; reason: string }
  | { kind: "compact" }
  | {
      kind: "result";
      subtype: string;
      isError: boolean;
      numTurns: number;
      durationMs: number;
      durationApiMs: number;
      totalCostUsd: number | null;
      resultText: string | null;
      permissionDenials: string[];
    }
  | { kind: "failed"; error: AppError };

export interface LiveTurn {
  turnId: string;
  sessionId: string | null;
  prompt: string;
  startedAt: string;
  blocks: Block[];
  running: boolean;
}

function findTool(blocks: Block[], toolUseId: string): number {
  return blocks.findIndex((b) => b.kind === "tool" && b.toolUseId === toolUseId);
}

function findText(blocks: Block[], blockIndex: number): number {
  return blocks.findIndex((b) => b.kind === "text" && b.blockIndex === blockIndex);
}

/** Folds one `ChatEvent` into `live`, returning a new `LiveTurn` (never mutates `live`). */
export function applyEvent(live: LiveTurn, event: ChatEvent): LiveTurn {
  const blocks = live.blocks.slice();
  let sessionId = live.sessionId;

  switch (event.type) {
    case "sessionReady": {
      sessionId = event.data.sessionId;
      break;
    }
    case "textDelta": {
      const idx = findText(blocks, event.data.blockIndex);
      if (idx === -1) {
        blocks.push({ kind: "text", blockIndex: event.data.blockIndex, text: event.data.text });
      } else {
        const existing = blocks[idx] as Extract<Block, { kind: "text" }>;
        blocks[idx] = { ...existing, text: existing.text + event.data.text };
      }
      break;
    }
    case "textBlock": {
      const idx = findText(blocks, event.data.blockIndex);
      const block: Block = { kind: "text", blockIndex: event.data.blockIndex, text: event.data.text };
      if (idx === -1) blocks.push(block);
      else blocks[idx] = block;
      break;
    }
    case "thinkingDelta":
      break; // rendered collapsed / not surfaced in M3's minimal UI
    case "toolCallStarted": {
      blocks.push({
        kind: "tool",
        toolUseId: event.data.toolUseId,
        name: event.data.name,
        inputPreview: event.data.inputPreview,
        input: undefined,
      });
      break;
    }
    case "toolCallInputDelta": {
      const idx = findTool(blocks, event.data.toolUseId);
      if (idx !== -1) {
        const existing = blocks[idx] as Extract<Block, { kind: "tool" }>;
        blocks[idx] = { ...existing, inputPreview: existing.inputPreview + event.data.partialJson };
      }
      break;
    }
    case "toolCallCompleted": {
      const idx = findTool(blocks, event.data.toolUseId);
      if (idx !== -1) {
        const existing = blocks[idx] as Extract<Block, { kind: "tool" }>;
        blocks[idx] = { ...existing, input: event.data.input };
      }
      break;
    }
    case "toolResult": {
      const idx = findTool(blocks, event.data.toolUseId);
      if (idx !== -1) {
        const existing = blocks[idx] as Extract<Block, { kind: "tool" }>;
        blocks[idx] = {
          ...existing,
          result: { isError: event.data.isError, summary: event.data.summary, full: event.data.full },
        };
      }
      break;
    }
    case "subagentMessage": {
      blocks.push({
        kind: "subagent",
        parentToolUseId: event.data.parentToolUseId,
        role: event.data.role,
        text: event.data.text,
      });
      break;
    }
    case "apiRetry": {
      blocks.push({ kind: "apiRetry", attempt: event.data.attempt, maxRetries: event.data.maxRetries, error: event.data.error });
      break;
    }
    case "permissionDenied": {
      blocks.push({ kind: "permissionDenied", tool: event.data.tool, reason: event.data.reason });
      break;
    }
    case "compactBoundary": {
      blocks.push({ kind: "compact" });
      break;
    }
    case "result": {
      blocks.push({
        kind: "result",
        subtype: event.data.subtype,
        isError: event.data.isError,
        numTurns: event.data.numTurns,
        durationMs: event.data.durationMs,
        durationApiMs: event.data.durationApiMs,
        totalCostUsd: event.data.totalCostUsd,
        resultText: event.data.resultText,
        permissionDenials: event.data.permissionDenials,
      });
      break;
    }
    case "changesComputed":
      break; // M4 — no snapshot subsystem yet
    case "failed": {
      blocks.push({ kind: "failed", error: event.data.error });
      break;
    }
    default:
      break; // forward-compat: an event kind this build doesn't know yet
  }

  const finished = event.type === "result" || event.type === "failed";
  return { ...live, blocks, sessionId, running: live.running && !finished };
}

/** Builds a `TurnRecord`-shaped entry from a finished `LiveTurn` so it can be prepended to
 * restored history without waiting on (and racing) the backend's own disk write. Fields
 * this app never reconstructs client-side (`model`, `policy`, `argv`, `events`,
 * `snapshotBefore`, `changes`) get inert placeholders — nothing in the history renderer
 * reads them. */
export function toTurnRecord(live: LiveTurn): TurnRecord {
  const assistantText = live.blocks
    .filter((b): b is Extract<Block, { kind: "text" }> => b.kind === "text")
    .map((b) => b.text)
    .join("");
  const toolCalls = live.blocks
    .filter((b): b is Extract<Block, { kind: "tool" }> => b.kind === "tool")
    .map((b) => ({
      toolUseId: b.toolUseId,
      name: b.name,
      input: (b.input ?? null) as JsonValue,
      isError: b.result?.isError ?? false,
    }));
  const resultBlock = live.blocks.find((b): b is Extract<Block, { kind: "result" }> => b.kind === "result");

  return {
    schemaVersion: 1,
    turnId: live.turnId,
    sessionId: live.sessionId ?? "",
    startedAt: live.startedAt,
    endedAt: new Date().toISOString(),
    prompt: live.prompt,
    attachments: [],
    model: "",
    policy: "guarded",
    argv: [],
    events: [],
    // Inert here too — the "Changes" button always re-fetches fresh via `changesForTurn`
    // rather than reading these off the (possibly synthetic, client-only) TurnRecord.
    snapshotBefore: null,
    changes: [],
    assistantText,
    toolCalls,
    result: resultBlock
      ? {
          subtype: resultBlock.subtype,
          isError: resultBlock.isError,
          numTurns: resultBlock.numTurns,
          durationMs: resultBlock.durationMs,
          durationApiMs: resultBlock.durationApiMs,
          totalCostUsd: resultBlock.totalCostUsd,
          permissionDenials: resultBlock.permissionDenials,
        }
      : null,
  };
}
