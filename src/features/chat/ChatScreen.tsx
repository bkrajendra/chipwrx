import { useEffect, useRef, useState } from "react";
import type { AppError, TurnRecord } from "../../lib/bindings";
import { ChangesPanel } from "../changes/ChangesPanel";
import { renderAppError } from "../../lib/errors";
import { PipelinePanel } from "../pipeline/PipelinePanel";
import { PipelineStrip } from "../pipeline/PipelineStrip";
import { usePipeline } from "../pipeline/usePipeline";
import { PermissionPolicyControl } from "../settings-global/PermissionPolicyControl";
import type { Block, LiveTurn } from "./blocks";
import { PromptDeck } from "./PromptDeck";
import { useChat } from "./useChat";

function formatCost(usd: number | null): string {
  if (usd === null) return "";
  return ` · ~$${usd.toFixed(4)} (est.)`;
}

function ResultFooter({
  numTurns,
  durationMs,
  totalCostUsd,
  isError,
  onOpenChanges,
}: {
  numTurns: number;
  durationMs: number;
  totalCostUsd: number | null;
  isError: boolean;
  onOpenChanges?: () => void;
}) {
  return (
    <div className="flex items-center justify-between gap-2">
      <p className={`text-xs ${isError ? "text-red-500" : "text-neutral-500 dark:text-neutral-400"}`}>
        {isError ? "Turn ended with an error" : "Done"} · {numTurns} turn{numTurns === 1 ? "" : "s"} · {(durationMs / 1000).toFixed(1)}s
        {formatCost(totalCostUsd)}
      </p>
      {onOpenChanges && (
        <button type="button" onClick={onOpenChanges} className="shrink-0 text-xs text-neutral-500 hover:underline dark:text-neutral-400">
          Changes
        </button>
      )}
    </div>
  );
}

function ToolCallCard({ block }: { block: Extract<Block, { kind: "tool" }> }) {
  const [expanded, setExpanded] = useState(false);
  const isError = block.result?.isError ?? false;
  return (
    <div className={`rounded border px-2.5 py-1.5 text-xs ${isError ? "border-red-300 dark:border-red-800" : "border-neutral-200 dark:border-neutral-800"}`}>
      <button type="button" onClick={() => setExpanded((v) => !v)} className="flex w-full items-center justify-between text-left">
        <span className="font-mono font-medium">{block.name}</span>
        <span className="text-neutral-400">{block.result ? (isError ? "failed" : "done") : "running…"}</span>
      </button>
      {expanded && (
        <div className="mt-1.5 space-y-1 border-t border-neutral-200 pt-1.5 dark:border-neutral-800">
          {block.input !== undefined && (
            <pre className="overflow-x-auto whitespace-pre-wrap text-[11px] text-neutral-600 dark:text-neutral-400">
              {JSON.stringify(block.input, null, 2)}
            </pre>
          )}
          {block.result && (
            <p className="whitespace-pre-wrap text-[11px] text-neutral-600 dark:text-neutral-400">
              {block.result.full ?? block.result.summary}
            </p>
          )}
        </div>
      )}
    </div>
  );
}

function describeFailure(error: AppError): string {
  const { title, message } = renderAppError(error);
  return `${title} — ${message}`;
}

function BlockView({ block }: { block: Block }) {
  switch (block.kind) {
    case "text":
      return <p className="whitespace-pre-wrap text-sm">{block.text}</p>;
    case "tool":
      return <ToolCallCard block={block} />;
    case "subagent":
      return (
        <p className="border-l-2 border-neutral-300 pl-2 text-xs italic text-neutral-500 dark:border-neutral-700 dark:text-neutral-400">
          [{block.role}] {block.text}
        </p>
      );
    case "apiRetry":
      return (
        <p className="w-fit rounded-full bg-amber-100 px-2.5 py-0.5 text-[11px] text-amber-800 dark:bg-amber-950 dark:text-amber-300">
          Retrying (attempt {block.attempt}) — {block.error}
        </p>
      );
    case "permissionDenied":
      return (
        <p className="rounded border border-amber-400 bg-amber-50 px-2.5 py-1.5 text-xs dark:border-amber-700 dark:bg-amber-950">
          Permission denied for <span className="font-mono">{block.tool}</span>
          {block.reason ? `: ${block.reason}` : ""}
        </p>
      );
    case "compact":
      return <p className="text-center text-[11px] text-neutral-400">— context compacted —</p>;
    case "result":
      return <ResultFooter numTurns={block.numTurns} durationMs={block.durationMs} totalCostUsd={block.totalCostUsd} isError={block.isError} />;
    case "failed":
      return <p className="rounded border border-red-300 bg-red-50 px-2.5 py-1.5 text-xs text-red-700 dark:border-red-800 dark:bg-red-950 dark:text-red-400">{describeFailure(block.error)}</p>;
    default:
      return null;
  }
}

function PromptBubble({ text }: { text: string }) {
  return (
    <div className="ml-auto max-w-[85%] rounded-lg bg-neutral-900 px-3 py-2 text-sm text-neutral-50 dark:bg-neutral-100 dark:text-neutral-900">
      {text}
    </div>
  );
}

function TurnHistoryCard({ turn, onOpenChanges }: { turn: TurnRecord; onOpenChanges: (turnId: string) => void }) {
  return (
    <div className="space-y-2">
      <PromptBubble text={turn.prompt} />
      {turn.assistantText && <p className="whitespace-pre-wrap text-sm">{turn.assistantText}</p>}
      {turn.toolCalls.map((tc) => (
        <div
          key={tc.toolUseId}
          className={`rounded border px-2.5 py-1.5 text-xs ${tc.isError ? "border-red-300 dark:border-red-800" : "border-neutral-200 dark:border-neutral-800"}`}
        >
          <span className="font-mono font-medium">{tc.name}</span>
        </div>
      ))}
      {turn.result && (
        <ResultFooter
          numTurns={turn.result.numTurns}
          durationMs={turn.result.durationMs}
          totalCostUsd={turn.result.totalCostUsd}
          isError={turn.result.isError}
          onOpenChanges={turn.turnId ? () => onOpenChanges(turn.turnId) : undefined}
        />
      )}
    </div>
  );
}

function LiveTurnCard({ live, onOpenChanges }: { live: LiveTurn; onOpenChanges: (turnId: string) => void }) {
  return (
    <div className="space-y-2">
      <PromptBubble text={live.prompt} />
      {live.blocks.length === 0 && live.running && <p className="text-sm text-neutral-400">Thinking…</p>}
      {live.blocks.map((b, i) =>
        b.kind === "result" ? (
          <ResultFooter
            key={i}
            numTurns={b.numTurns}
            durationMs={b.durationMs}
            totalCostUsd={b.totalCostUsd}
            isError={b.isError}
            onOpenChanges={live.turnId ? () => onOpenChanges(live.turnId) : undefined}
          />
        ) : (
          <BlockView key={i} block={b} />
        ),
      )}
    </div>
  );
}

type SidePanel = { kind: "changes"; turnId: string } | { kind: "pipeline" };

export function ChatScreen({ workspaceId, workspaceName, workspacePath }: { workspaceId: string; workspaceName: string; workspacePath: string }) {
  const { history, live, loadingHistory, error, send, stop, newSession } = useChat(workspaceId);
  const pipeline = usePipeline(workspaceId);
  const bottomRef = useRef<HTMLDivElement>(null);
  const [sidePanel, setSidePanel] = useState<SidePanel | null>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ block: "end" });
  }, [history, live]);

  const running = live?.running ?? false;

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center justify-between border-b border-neutral-200 px-4 py-2 dark:border-neutral-800">
        <span className="text-sm font-medium">{workspaceName}</span>
        <div className="flex items-center gap-2">
          <PermissionPolicyControl workspacePath={workspacePath} />
          <PipelineStrip state={pipeline.state} onStop={pipeline.stop} onOpen={() => setSidePanel({ kind: "pipeline" })} />
          <button
            type="button"
            onClick={() => void newSession()}
            disabled={running}
            className="rounded border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            New session
          </button>
        </div>
      </header>

      <div className="flex flex-1 overflow-hidden">
        <div className="flex flex-1 flex-col overflow-hidden">
          <div className="flex-1 overflow-y-auto px-4 py-4">
            <div className="mx-auto max-w-2xl space-y-4">
              {loadingHistory && history.length === 0 && <p className="text-sm text-neutral-500 dark:text-neutral-400">Loading conversation…</p>}
              {!loadingHistory && history.length === 0 && !live && (
                <p className="text-sm text-neutral-500 dark:text-neutral-400">Say what you'd like to build.</p>
              )}
              {history.map((t) => (
                <TurnHistoryCard key={t.turnId} turn={t} onOpenChanges={(turnId) => setSidePanel({ kind: "changes", turnId })} />
              ))}
              {live && <LiveTurnCard live={live} onOpenChanges={(turnId) => setSidePanel({ kind: "changes", turnId })} />}
              <div ref={bottomRef} />
            </div>
          </div>

          {error && <p className="px-4 pb-2 text-xs text-red-500">{error}</p>}

          <div className="mx-auto w-full max-w-2xl">
            <PromptDeck running={running} history={history.map((t) => t.prompt)} onSend={(text) => void send(text)} onStop={stop} />
          </div>
        </div>

        {sidePanel?.kind === "changes" && (
          <div className="w-[440px] shrink-0 border-l border-neutral-200 dark:border-neutral-800">
            <ChangesPanel workspaceId={workspaceId} turnId={sidePanel.turnId} onClose={() => setSidePanel(null)} />
          </div>
        )}
        {sidePanel?.kind === "pipeline" && (
          <div className="w-[520px] shrink-0 border-l border-neutral-200 dark:border-neutral-800">
            <PipelinePanel
              workspaceId={workspaceId}
              pipeline={pipeline}
              onClose={() => setSidePanel(null)}
              onAskClaudeToFix={(prompt) => {
                setSidePanel(null);
                void send(prompt);
              }}
            />
          </div>
        )}
      </div>
    </div>
  );
}
