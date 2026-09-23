// Canvas "Problems" tab (`FR-UI-3`, `FR-BUILD-4/9`): structured defects from build/check.

import type { Defect } from "../../lib/bindings";
import type { LogLineView } from "./usePipeline";
import { composeFixPrompt } from "./fixPrompt";

const SEVERITY_COLOR: Record<Defect["severity"], string> = {
  error: "text-red-600 dark:text-red-400",
  warning: "text-amber-600 dark:text-amber-400",
  note: "text-neutral-500 dark:text-neutral-400",
};

// `NFR-A2`: status conveyed by icon + text, never colour alone — each row's severity is
// also spelled out as a word, not just a colour.
const SEVERITY_LABEL: Record<Defect["severity"], string> = {
  error: "Error",
  warning: "Warning",
  note: "Note",
};

export function ProblemsTab({
  defects,
  lines,
  running,
  onAskClaudeToFix,
  onRunCheck,
  checking,
}: {
  defects: Defect[];
  lines: LogLineView[];
  running: boolean;
  onAskClaudeToFix: (prompt: string) => void;
  onRunCheck: () => void;
  checking: boolean;
}) {
  const errorCount = defects.filter((d) => d.severity === "error").length;

  return (
    <div className="h-full overflow-y-auto px-4 py-2">
      <div className="mb-2 flex items-center gap-2">
        {!running && errorCount > 0 && (
          <button
            type="button"
            onClick={() => onAskClaudeToFix(composeFixPrompt(defects, lines))}
            className="rounded bg-red-600 px-2.5 py-1.5 text-xs font-medium text-white hover:bg-red-700"
          >
            Ask Claude to fix these {errorCount} error{errorCount === 1 ? "" : "s"}
          </button>
        )}
        <button
          type="button"
          onClick={onRunCheck}
          disabled={checking}
          title="pio check --json-output"
          className="rounded border border-neutral-300 px-2.5 py-1.5 text-xs font-medium hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800"
        >
          {checking ? "Running static analysis…" : "Run static analysis"}
        </button>
      </div>
      {defects.length === 0 && <p className="text-xs text-neutral-500 dark:text-neutral-400">No problems.</p>}
      {defects.map((d, i) => (
        <div key={i} className="border-b border-neutral-100 py-1.5 text-xs last:border-0 dark:border-neutral-900">
          <span className={`font-mono font-medium ${SEVERITY_COLOR[d.severity]}`}>
            [{SEVERITY_LABEL[d.severity]}] {d.file}:{d.line}
            {d.column ? `:${d.column}` : ""}
          </span>
          {d.source === "check" && (
            <span className="ml-1.5 rounded bg-neutral-100 px-1 py-0.5 text-[10px] font-medium text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400">
              check
            </span>
          )}
          <span className="ml-1.5 text-neutral-700 dark:text-neutral-300">{d.message}</span>
        </div>
      ))}
    </div>
  );
}
