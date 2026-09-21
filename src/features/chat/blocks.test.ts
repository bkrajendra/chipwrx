import { describe, expect, it } from "vitest";
import type { ChatEvent } from "../../lib/bindings";
import { applyEvent, toTurnRecord, type Block, type LiveTurn } from "./blocks";

function live(): LiveTurn {
  return { turnId: "t1", sessionId: null, prompt: "add a blink sketch", startedAt: "2026-09-21T00:00:00.000Z", blocks: [], running: true };
}

describe("applyEvent", () => {
  it("accumulates textDelta chunks into one block by blockIndex", () => {
    let l = live();
    l = applyEvent(l, { type: "textDelta", data: { turnId: "t1", blockIndex: 0, text: "Hello" } });
    l = applyEvent(l, { type: "textDelta", data: { turnId: "t1", blockIndex: 0, text: " world" } });
    expect(l.blocks).toEqual<Block[]>([{ kind: "text", blockIndex: 0, text: "Hello world" }]);
    expect(l.running).toBe(true);
  });

  it("textBlock replaces the accumulated text for its blockIndex", () => {
    let l = live();
    l = applyEvent(l, { type: "textDelta", data: { turnId: "t1", blockIndex: 0, text: "Hel" } });
    l = applyEvent(l, { type: "textBlock", data: { turnId: "t1", blockIndex: 0, text: "Hello world" } });
    expect(l.blocks).toEqual<Block[]>([{ kind: "text", blockIndex: 0, text: "Hello world" }]);
  });

  it("carries a tool call through started -> inputDelta -> completed -> result", () => {
    let l = live();
    l = applyEvent(l, { type: "toolCallStarted", data: { turnId: "t1", toolUseId: "tu1", name: "Write", inputPreview: "" } });
    l = applyEvent(l, { type: "toolCallInputDelta", data: { turnId: "t1", toolUseId: "tu1", partialJson: '{"file_path":' } });
    l = applyEvent(l, {
      type: "toolCallCompleted",
      data: { turnId: "t1", toolUseId: "tu1", name: "Write", input: { file_path: "src/main.cpp" } },
    });
    l = applyEvent(l, {
      type: "toolResult",
      data: { turnId: "t1", toolUseId: "tu1", isError: false, summary: "File written successfully.", full: "File written successfully." },
    });

    expect(l.blocks).toHaveLength(1);
    const block = l.blocks[0] as Extract<Block, { kind: "tool" }>;
    expect(block.kind).toBe("tool");
    expect(block.name).toBe("Write");
    expect(block.input).toEqual({ file_path: "src/main.cpp" });
    expect(block.result).toEqual({ isError: false, summary: "File written successfully.", full: "File written successfully." });
  });

  it("pushes apiRetry and permissionDenied as their own blocks", () => {
    let l = live();
    l = applyEvent(l, {
      type: "apiRetry",
      data: { turnId: "t1", attempt: 1, maxRetries: 5, retryDelayMs: 2000, error: "overloaded", errorStatus: 529 },
    });
    l = applyEvent(l, { type: "permissionDenied", data: { turnId: "t1", tool: "Bash", reason: "not allowed" } });
    expect(l.blocks).toEqual<Block[]>([
      { kind: "apiRetry", attempt: 1, maxRetries: 5, error: "overloaded" },
      { kind: "permissionDenied", tool: "Bash", reason: "not allowed" },
    ]);
  });

  it("result ends the turn", () => {
    const resultEvent: ChatEvent = {
      type: "result",
      data: {
        turnId: "t1",
        sessionId: "s1",
        subtype: "success",
        isError: false,
        numTurns: 1,
        durationMs: 4213,
        durationApiMs: 3190,
        totalCostUsd: 0.0431,
        resultText: "Added a blink sketch.",
        permissionDenials: [],
      },
    };
    const l = applyEvent(live(), resultEvent);
    expect(l.running).toBe(false);
    expect(l.blocks[0]).toMatchObject({ kind: "result", numTurns: 1, isError: false });
  });

  it("failed ends the turn without a result block", () => {
    const l = applyEvent(live(), {
      type: "failed",
      data: { turnId: "t1", error: { code: "CLAUDE_INTERRUPTED", sessionId: "s1" } },
    });
    expect(l.running).toBe(false);
    expect(l.blocks[0]).toMatchObject({ kind: "failed" });
  });

  it("once finished, running never flips back to true", () => {
    let l = applyEvent(live(), {
      type: "failed",
      data: { turnId: "t1", error: { code: "CLAUDE_INTERRUPTED", sessionId: "s1" } },
    });
    l = applyEvent(l, { type: "textDelta", data: { turnId: "t1", blockIndex: 0, text: "late" } });
    expect(l.running).toBe(false);
  });
});

describe("toTurnRecord", () => {
  it("reconstructs assistantText, toolCalls, and result from a finished live turn", () => {
    let l = live();
    l = applyEvent(l, {
      type: "sessionReady",
      data: { sessionId: "s1", model: "sonnet", tools: [], capabilities: [], mcpErrors: [], pluginErrors: [] },
    });
    l = applyEvent(l, { type: "textBlock", data: { turnId: "t1", blockIndex: 0, text: "Done." } });
    l = applyEvent(l, { type: "toolCallStarted", data: { turnId: "t1", toolUseId: "tu1", name: "Write", inputPreview: "" } });
    l = applyEvent(l, { type: "toolCallCompleted", data: { turnId: "t1", toolUseId: "tu1", name: "Write", input: { a: 1 } } });
    l = applyEvent(l, { type: "toolResult", data: { turnId: "t1", toolUseId: "tu1", isError: false, summary: "ok", full: "ok" } });
    l = applyEvent(l, {
      type: "result",
      data: {
        turnId: "t1",
        sessionId: "s1",
        subtype: "success",
        isError: false,
        numTurns: 1,
        durationMs: 100,
        durationApiMs: 80,
        totalCostUsd: 0.01,
        resultText: "Done.",
        permissionDenials: [],
      },
    });

    const record = toTurnRecord(l);
    expect(record.turnId).toBe("t1");
    expect(record.sessionId).toBe("s1");
    expect(record.prompt).toBe("add a blink sketch");
    expect(record.assistantText).toBe("Done.");
    expect(record.toolCalls).toEqual([{ toolUseId: "tu1", name: "Write", input: { a: 1 }, isError: false }]);
    expect(record.result).toMatchObject({ subtype: "success", numTurns: 1, totalCostUsd: 0.01 });
  });

  it("has no result when the turn failed instead", () => {
    const l = applyEvent(live(), {
      type: "failed",
      data: { turnId: "t1", error: { code: "CLAUDE_INTERRUPTED", sessionId: "s1" } },
    });
    const record = toTurnRecord(l);
    expect(record.result).toBeNull();
  });
});
